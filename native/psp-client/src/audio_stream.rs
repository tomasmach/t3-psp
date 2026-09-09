use crate::{
    network,
    stream_queue::{self, BLOCK_BYTES, Consumer, Producer},
};
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    sync::Arc,
};
use core::{
    cell::{Cell, UnsafeCell},
    ffi::c_void,
    marker::PhantomData,
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};
use psp::sys::*;

const SAMPLES: usize = BLOCK_BYTES / 2;
// rust-psp 0.3.13 drops this firmware return value. Keep its import, but
// call the three-argument stub with the actual PSPSDK signature.
#[used]
static AUDIO_INPUT_IMPORT: unsafe extern "C" fn(i32, AudioInputFrequency, *mut c_void) =
    sceAudioInput;
unsafe extern "C" {
    fn __sceAudioInput_stub(
        samples: i32,
        frequency: AudioInputFrequency,
        buffer: *mut c_void,
    ) -> i32;
}
static ACTIVE: AtomicBool = AtomicBool::new(false);
static POISONED: AtomicBool = AtomicBool::new(false);
// Firmware writes asynchronously. This storage is never freed, including timeout paths.
static mut CAPTURE: [i16; SAMPLES] = [0; SAMPLES];

struct Shared {
    uploaded_blocks: AtomicU32,
    done: AtomicBool,
    error: UnsafeCell<Option<String>>,
}
// Only the uploader writes error, before releasing done. The main thread reads
// it only after acquiring done, at which point it is immutable.
unsafe impl Sync for Shared {}

struct Worker {
    config: network::Config,
    id: String,
    queue: Consumer,
    shared: Arc<Shared>,
}

unsafe extern "C" fn upload_worker(_size: usize, arguments: *mut c_void) -> i32 {
    let pointer = unsafe { *(arguments.cast::<*mut Worker>()) };
    let mut worker = unsafe { Box::from_raw(pointer) };
    let mut bytes = [0u8; BLOCK_BYTES];
    let mut sequence = 0u32;
    let result = (|| {
        loop {
            if worker.queue.finished() {
                return Ok(());
            }
            if !worker.queue.pop(&mut bytes) {
                unsafe {
                    sceKernelDelayThread(5_000);
                }
                continue;
            }
            if worker.queue.cancelled() {
                return Ok(());
            }
            let path = format!("/v1/recordings/{}/chunks?sequence={sequence}", worker.id);
            // Never retry an ambiguous POST. The caller cancels the recording on error.
            let reply = network::request(&worker.config, &path, &bytes, true)?;
            let next_sequence = sequence
                .checked_add(1)
                .ok_or_else(|| String::from("Recording sequence exhausted"))?;
            let expected = format!("OK\t{next_sequence}");
            if reply.trim_end_matches(['\r', '\n']) != expected {
                return Err(String::from("Unexpected recording chunk acknowledgement"));
            }
            sequence = next_sequence;
            worker
                .shared
                .uploaded_blocks
                .store(sequence, Ordering::Release);
        }
    })();
    unsafe {
        *worker.shared.error.get() = result.err();
    }
    worker.shared.done.store(true, Ordering::Release);
    0
}

pub struct Progress {
    pub captured_samples: u64,
    pub uploaded_samples: u64,
    pub stopping: bool,
}

pub struct Session {
    queue: Producer,
    shared: Arc<Shared>,
    thread: Cell<Option<SceUid>>,
    capturing: bool,
    started: i64,
    captured: u64,
    stopping: bool,
    cancelled: bool,
    failed: bool,
    worker_observed: bool,
    start_error: Option<i32>,
    // All capture operations belong to the PSP main thread.
    _main_thread: PhantomData<*mut ()>,
}

impl Session {
    pub fn start(config: &network::Config, id: &str) -> Result<Self, String> {
        if id.is_empty()
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(String::from("Invalid recording ID"));
        }
        if POISONED.load(Ordering::Acquire) {
            return Err(String::from("Microphone capture failed. Restart T3 PSP."));
        }
        if ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(String::from("A recording is already active"));
        }
        // 0x40 clipped 2.1% of samples in a real PSP-3000 speech recording.
        let init = unsafe { sceAudioInputInit(0, 0x20, 0) };
        if init < 0 {
            ACTIVE.store(false, Ordering::Release);
            return Err(format!("Microphone init: {init:#x}"));
        }
        let (queue, consumer) = stream_queue::channel();
        let shared = Arc::new(Shared {
            uploaded_blocks: AtomicU32::new(0),
            done: AtomicBool::new(false),
            error: UnsafeCell::new(None),
        });
        let worker = Box::new(Worker {
            config: config.clone(),
            id: id.to_string(),
            queue: consumer,
            shared: shared.clone(),
        });
        let thread = unsafe {
            sceKernelCreateThread(
                c"T3 audio upload".as_ptr().cast(),
                upload_worker,
                0x20,
                96 * 1024,
                ThreadAttributes::USER,
                core::ptr::null_mut(),
            )
        };
        if thread.0 < 0 {
            ACTIVE.store(false, Ordering::Release);
            return Err(format!("Create upload thread: {:#x}", thread.0));
        }
        let mut pointer = Box::into_raw(worker);
        let start = unsafe {
            sceKernelStartThread(
                thread,
                core::mem::size_of::<*mut Worker>(),
                (&raw mut pointer).cast(),
            )
        };
        if start < 0 {
            unsafe {
                drop(Box::from_raw(pointer));
                sceKernelDeleteThread(thread);
            }
            ACTIVE.store(false, Ordering::Release);
            return Err(format!("Start upload thread: {start:#x}"));
        }
        let mut session = Self {
            queue,
            shared,
            thread: Cell::new(Some(thread)),
            capturing: false,
            started: 0,
            captured: 0,
            stopping: false,
            cancelled: false,
            failed: false,
            worker_observed: false,
            start_error: None,
            _main_thread: PhantomData,
        };
        session.begin_block();
        Ok(session)
    }

    fn begin_block(&mut self) {
        let buffer = (&raw mut CAPTURE).cast::<i16>();
        unsafe {
            core::ptr::write_bytes(buffer, 0, SAMPLES);
            self.started = sceKernelGetSystemTimeWide();
            let result = __sceAudioInput_stub(
                SAMPLES as i32,
                AudioInputFrequency::Khz11_025,
                buffer.cast(),
            );
            self.capturing = result >= 0;
            self.start_error = (result < 0).then_some(result);
        }
    }

    fn failure(&mut self, message: String) -> Result<Progress, String> {
        self.failed = true;
        self.cancel();
        Err(message)
    }

    /// Poll from the UI loop, including while stopping or after an error.
    /// Does not wait for the microphone, network, or uploader thread.
    pub fn tick(&mut self) -> Result<Progress, String> {
        if let Some(error) = self.start_error.take() {
            return self.failure(format!("Microphone capture failed to start: {error:#x}"));
        }
        // finished() must not race the last HTTP result: the UI has to observe
        // the worker's error through tick before it can finalize a recording.
        if !self.worker_observed && self.shared.done.load(Ordering::Acquire) {
            self.worker_observed = true;
            let error = unsafe { (&*self.shared.error.get()).clone() };
            if !self.failed {
                if let Some(error) = error {
                    return self.failure(format!("Audio upload failed: {error}"));
                }
            }
        }
        if self.capturing {
            let state = unsafe { sceAudioPollInputEnd() };
            if state == 0 {
                self.capturing = false;
                if !self.cancelled {
                    let acquired = unsafe { sceAudioGetInputLength() };
                    if acquired != SAMPLES as i32 {
                        return self
                            .failure(format!("Incomplete microphone block: {acquired}/{SAMPLES}"));
                    }
                    self.captured += SAMPLES as u64;
                    let bytes = unsafe { &*((&raw const CAPTURE).cast::<[u8; BLOCK_BYTES]>()) };
                    if self.queue.push(bytes).is_err() {
                        return self.failure(String::from("Audio upload cannot keep up. Recording cancelled; no incomplete transcript will be sent."));
                    }
                    if !self.stopping {
                        self.begin_block();
                    }
                }
                if self.stopping {
                    self.queue.close();
                }
            } else if !self.failed {
                let elapsed = unsafe { sceKernelGetSystemTimeWide() } - self.started;
                if state < 0 || elapsed >= 10_000_000 {
                    POISONED.store(true, Ordering::Release);
                    return self.failure(format!(
                        "Microphone capture did not finish ({state:#x}). Restart T3 PSP."
                    ));
                }
            }
        }
        Ok(self.progress())
    }

    pub fn progress(&self) -> Progress {
        Progress {
            captured_samples: self.captured,
            uploaded_samples: self.shared.uploaded_blocks.load(Ordering::Acquire) as u64
                * SAMPLES as u64,
            stopping: self.stopping,
        }
    }

    /// Finish the current 371 ms block, then let the worker drain queued audio.
    pub fn stop(&mut self) {
        self.stopping = true;
        if !self.capturing {
            self.queue.close();
        }
    }

    /// An already-running HTTP request may finish; queued chunks are discarded.
    pub fn cancel(&mut self) {
        self.stopping = true;
        self.cancelled = true;
        self.queue.cancel();
    }

    pub fn finished(&self) -> bool {
        if self.capturing || !self.worker_observed {
            return false;
        }
        if let Some(thread) = self.thread.get() {
            // done is published just before callback return. Check actual kernel
            // exit as well before deleting this session's own thread handle.
            if unsafe { sceKernelGetThreadExitStatus(thread) } != 0 {
                return false;
            }
            if unsafe { sceKernelDeleteThread(thread) } < 0 {
                return false;
            }
            self.thread.set(None);
        }
        true
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.cancel();
        if !self.finished() {
            // The UI normally keeps ticking until finished. If it abandons a
            // session, keep raw storage alive and refuse unsafe future reuse.
            POISONED.store(true, Ordering::Release);
        }
        ACTIVE.store(false, Ordering::Release);
    }
}
