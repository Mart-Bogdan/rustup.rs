use lazy_static::lazy::*;

#[cfg(windows)]
use {
    winapi::shared::minwindef::DWORD,
    winapi::um::winbase::{STD_ERROR_HANDLE, STD_OUTPUT_HANDLE},
};

static MEMOIZED_STDOUT_IS_TTY: Lazy<bool> = Lazy::INIT;
static MEMOIZED_STDERR_IS_TTY: Lazy<bool> = Lazy::INIT;

#[cfg(unix)]
const STDOUT_FD: libc::c_int = libc::STDOUT_FILENO;
#[cfg(unix)]
const STDERR_FD: libc::c_int = libc::STDERR_FILENO;

#[cfg(windows)]
const STDOUT_FD: DWORD = STD_OUTPUT_HANDLE;
#[cfg(windows)]
const STDERR_FD: DWORD = STD_ERROR_HANDLE;

// Originally copied from rustc. atty crate did not work as expected
pub fn stderr_isatty() -> bool {
    // memoize call, as it currently gets called on each output
    // and it won't change during program execution
    let res = MEMOIZED_STDERR_IS_TTY.get(|| isatty(STDERR_FD));
    *res
}

pub fn stdout_isatty() -> bool {
    // memoize call, as it currently gets called on each output
    // and it won't change during program execution
    let res = MEMOIZED_STDOUT_IS_TTY.get(|| isatty(STDOUT_FD));
    *res
}

#[inline]
#[cfg(unix)]
fn isatty(fd: libc::c_int) -> bool {
    unsafe { libc::isatty(fd) == 1 }
}

#[inline]
#[cfg(windows)]
fn isatty(fd: winapi::shared::minwindef::DWORD) -> bool {
    win::isatty(fd)
}

//separate sub-module to not use [cfg(windows)] on each definition
#[cfg(windows)]
mod win {

    use std::ffi::OsString;

    use std::os::windows::prelude::*;
    use winapi::shared::minwindef::{DWORD, FALSE};
    use winapi::shared::ntdef::HANDLE;
    use winapi::um::consoleapi::{GetConsoleMode, SetConsoleMode};
    use winapi::um::fileapi::GetFileType;

    use winapi::um::processenv::GetStdHandle;

    use winapi::um::winbase::FILE_TYPE_PIPE;
    use winapi::um::wincon::ENABLE_VIRTUAL_TERMINAL_PROCESSING;

    use winapi::shared::minwindef::BOOL;
    use winapi::shared::minwindef::LPVOID;

    use winapi::shared::ntdef::WCHAR;

    use winapi::um::minwinbase::FileNameInfo;
    use winapi::um::winbase::GetFileInformationByHandleEx;

    /// Detects if it is real Win10+ console with VT support, or are we connected to
    /// console emulator, like Cygwin,Msys,GitBash,ConEmu, so we can use VT100 sequences
    /// Windows consoles prior to Win10 and file redirections gets uncolored output.
    pub fn isatty(fd: winapi::shared::minwindef::DWORD) -> bool {
        unsafe {
            let handle: HANDLE = GetStdHandle(fd);
            if handle.is_null() {
                //we do not have attached console
                return false;
            }
            let mut console_mode = 0;
            let is_a_tty = GetConsoleMode(handle, &mut console_mode) != FALSE;
            if is_a_tty {
                //we are calling this to enable VT100 escapes on WINDOWS 10+
                //if we are unable to call this method, than we are on OS prior to Win10, and VT100
                // is unavailable, so we must behave like there are no tty -- no color output!
                return SetConsoleMode(handle, console_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING)
                    != FALSE;
            } else {
                // If input redirected not to pipe
                if GetFileType(handle) != FILE_TYPE_PIPE {
                    return false;
                }

                return is_win_console_emulators(handle);
            }
        }
    }

    fn is_win_console_emulators(handle: HANDLE) -> bool {
        match get_file_name_by_handle(handle) {
            Option::Some(name) => {
                /*
                 * MSYS2 pty pipe ('\msys-XXXX-ptyN-XX')
                 * cygwin pty pipe ('\cygwin-XXXX-ptyN-XX')
                 * ConEmu pty pipe ('\ConEmuHk****')
                 */
                name.starts_with("\\ConEmuHk")
                    || name.starts_with("\\cygwin-")
                    || name.starts_with("\\msys-")
            }
            None => false,
        }
    }

    /// see https://docs.microsoft.com/en-us/windows/desktop/api/winbase/ns-winbase-_file_name_info
    /// not using winapi::um::fileapi::FILE_NAME_INFO as it would force us to work with transmute
    /// and unsafe casts/unsafe pointers etc. 2040 would suffice for us.
    #[repr(C)]
    #[allow(non_snake_case)]
    struct FileNameInfo {
        FileNameLength: DWORD,
        /// 2040 to allow it combined with length to be not more than 2048. Con emulators pipe names
        /// won't be larger than that!
        FileName: [WCHAR; 2040],
    }

    fn get_file_name_by_handle(handle: HANDLE) -> Option<String> {
        unsafe {
            let mut name_buf = FileNameInfo {
                FileNameLength: 0,
                FileName: [0; 2040],
            };

            //https://docs.microsoft.com/en-us/windows/desktop/api/winbase/nf-winbase-getfileinformationbyhandleex
            //Works only on Win past Vista, but rustup anyeay won't run on XP
            // as it uses RegOpenKeyTransactedA and fails to run on XP with runtime link error!
            //Tried using GetFinalPathNameByHandleW, but it fails with error code 1 (ERROR_INVALID_FUNCTION)
            // on my Win 10 box, despite fact that this function returns nice full qualified path.
            let res: BOOL = GetFileInformationByHandleEx(
                handle,
                FileNameInfo,
                &mut name_buf as *mut _ as LPVOID,
                ::std::mem::size_of::<FileNameInfo>() as u32,
            );
            if res == 0 {
                return Option::None;
            }
            let wide = &name_buf.FileName
                [0..((name_buf.FileNameLength / 2)/*size in byetes(!)*/ as usize)];
            let str = OsString::from_wide(wide);

            Option::from(str.to_string_lossy().into_owned())
        }
    }
}
