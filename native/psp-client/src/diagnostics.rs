use psp::sys::*;

pub fn display_error(operation: &str, code: u32) -> ! {
    psp::dprintln!("Display error: {} {:#x}\nHOME exit", operation, code);
    loop {
        unsafe {
            sceKernelDelayThreadCB(100_000);
        }
    }
}
