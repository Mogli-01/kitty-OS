#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]

#[cfg(not(target_os = "none"))]
fn main() {}

#[cfg(target_os = "none")]
mod guest {
    use core::arch::{asm, global_asm};
    use core::panic::PanicInfo;
    use kitty_shell::{Console, run_console};

    const EXIT: u64 = 0;
    const CONSOLE_READ: u64 = 6;
    const CONSOLE_WRITE: u64 = 5;
    const OPEN_AT: u64 = 9;
    const FILE_READ: u64 = 10;
    const HANDLE_CLOSE: u64 = 2;
    const CONSOLE: u64 = 0;
    const DIR: u64 = 1;
    const READ: u64 = 1;

    global_asm!(
        ".global _start",
        "_start:",
        "call {main}",
        "ud2",
        main = sym start,
    );

    extern "C" fn start() -> ! {
        exit(if run_console(&mut GuestConsole).is_ok() {
            0
        } else {
            1
        })
    }

    fn syscall(number: u64, a: u64, b: u64, c: u64, d: u64) -> Result<u64, i64> {
        let result: i64;
        // SAFETY: the documented kitty-OS ABI clobbers RCX and R11 and
        // returns RAX. Callers keep their slices alive for the entire
        // synchronous syscall. The kernel validates every range.
        unsafe {
            asm!(
                "syscall",
                inlateout("rax") number => result,
                in("rdi") a,
                in("rsi") b,
                in("rdx") c,
                in("r10") d,
                lateout("rcx") _,
                lateout("r11") _,
                options(nostack),
            );
        }
        u64::try_from(result).map_err(|_| result)
    }

    fn exit(status: u64) -> ! {
        let _ = syscall(EXIT, status, 0, 0, 0);
        loop {
            core::hint::spin_loop();
        }
    }

    struct GuestConsole;

    impl Console for GuestConsole {
        type Error = i64;

        fn read(&mut self, buffer: &mut [u8]) -> Result<usize, Self::Error> {
            syscall(
                CONSOLE_READ,
                CONSOLE,
                buffer.as_mut_ptr() as u64,
                buffer.len() as u64,
                0,
            )
            .map(|count| count as usize)
        }

        fn write(&mut self, bytes: &[u8]) -> Result<usize, Self::Error> {
            syscall(
                CONSOLE_WRITE,
                CONSOLE,
                bytes.as_ptr() as u64,
                bytes.len() as u64,
                0,
            )
            .map(|count| count as usize)
        }

        fn echo_input(&self) -> bool {
            true
        }

        fn cat(&mut self, path: &str) -> Option<Result<(), Self::Error>> {
            Some(self.cat_file(path))
        }
    }

    impl GuestConsole {
        fn cat_file(&mut self, path: &str) -> Result<(), i64> {
            let handle = match syscall(OPEN_AT, DIR, path.as_ptr() as u64, path.len() as u64, READ)
            {
                Ok(handle) => handle,
                Err(-8) => return self.write_full(b"file not found\n"),
                Err(-4) => return self.write_full(b"permission denied\n"),
                Err(-1) => return self.write_full(b"invalid relative path\n"),
                Err(_) => return self.write_full(b"could not open file\n"),
            };
            let result = self.stream_file(handle);
            let closed = syscall(HANDLE_CLOSE, handle, 0, 0, 0);
            result.and(closed.map(|_| ()))
        }

        fn stream_file(&mut self, handle: u64) -> Result<(), i64> {
            let mut buffer = [0u8; 512];
            let mut offset = 0u64;
            loop {
                let count = match syscall(
                    FILE_READ,
                    handle,
                    buffer.as_mut_ptr() as u64,
                    buffer.len() as u64,
                    offset,
                ) {
                    Ok(count) => count,
                    Err(_) => return self.write_full(b"could not read file\n"),
                };
                if count == 0 {
                    return Ok(());
                }
                let bytes = buffer.get(..count as usize).ok_or(-1)?;
                self.write_full(bytes)?;
                offset = offset.checked_add(count).ok_or(-1)?;
            }
        }

        fn write_full(&mut self, mut bytes: &[u8]) -> Result<(), i64> {
            while !bytes.is_empty() {
                let count = self.write(bytes)?;
                if count == 0 {
                    return Err(-1);
                }
                bytes = bytes.get(count..).ok_or(-1)?;
            }
            Ok(())
        }
    }

    #[panic_handler]
    fn panic(_: &PanicInfo<'_>) -> ! {
        exit(1)
    }
}
