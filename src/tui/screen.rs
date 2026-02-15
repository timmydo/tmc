use std::io::{self, BufWriter, Stdout, Write};
use std::os::unix::io::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};

static RESIZE_REQUESTED: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_sigwinch(_: libc::c_int) {
    RESIZE_REQUESTED.store(true, Ordering::Relaxed);
}

pub struct Terminal {
    original_termios: libc::termios,
    out: BufWriter<Stdout>,
    pub rows: u16,
    pub cols: u16,
    mouse: bool,
}

impl Terminal {
    pub fn new(mouse: bool) -> io::Result<Self> {
        let stdin_fd = io::stdin().as_raw_fd();

        // Save original termios
        let mut original_termios: libc::termios = unsafe { std::mem::zeroed() };
        if unsafe { libc::tcgetattr(stdin_fd, &mut original_termios) } == -1 {
            return Err(io::Error::last_os_error());
        }

        // Enable raw mode
        let mut raw = original_termios;
        raw.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        raw.c_oflag &= !libc::OPOST;
        raw.c_cflag |= libc::CS8;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);
        raw.c_cc[libc::VMIN] = 0;
        raw.c_cc[libc::VTIME] = 1; // 100ms timeout

        if unsafe { libc::tcsetattr(stdin_fd, libc::TCSAFLUSH, &raw) } == -1 {
            return Err(io::Error::last_os_error());
        }

        // Setup SIGWINCH handler
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = handle_sigwinch as libc::sighandler_t;
            libc::sigemptyset(&mut sa.sa_mask);
            sa.sa_flags = 0;
            libc::sigaction(libc::SIGWINCH, &sa, std::ptr::null_mut());
        }

        let (rows, cols) = get_window_size()?;

        let mut out = BufWriter::new(io::stdout());
        // Enter alternate screen buffer, hide cursor
        write!(out, "\x1b[?1049h\x1b[?25l")?;
        if mouse {
            // Enable X10 mouse tracking + SGR extended coordinates
            write!(out, "\x1b[?1000h\x1b[?1006h")?;
        }
        out.flush()?;

        Ok(Terminal {
            original_termios,
            out,
            rows,
            cols,
            mouse,
        })
    }

    /// Check if a resize was signaled and update dimensions.
    pub fn check_resize(&mut self) -> bool {
        if RESIZE_REQUESTED.swap(false, Ordering::Relaxed) {
            if let Ok((rows, cols)) = get_window_size() {
                self.rows = rows;
                self.cols = cols;
                return true;
            }
        }
        false
    }

    pub fn clear(&mut self) -> io::Result<()> {
        write!(self.out, "\x1b[2J\x1b[H")
    }

    pub fn move_to(&mut self, row: u16, col: u16) -> io::Result<()> {
        write!(self.out, "\x1b[{};{}H", row, col)
    }

    pub fn clear_line(&mut self) -> io::Result<()> {
        write!(self.out, "\x1b[K")
    }

    pub fn write_str(&mut self, s: &str) -> io::Result<()> {
        write!(self.out, "{}", s)
    }

    pub fn set_reverse(&mut self) -> io::Result<()> {
        write!(self.out, "\x1b[7m")
    }

    pub fn set_bold(&mut self) -> io::Result<()> {
        write!(self.out, "\x1b[1m")
    }

    pub fn reset_attr(&mut self) -> io::Result<()> {
        write!(self.out, "\x1b[0m")
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }

    /// Write a string truncated to fit within `max_width` columns.
    pub fn write_truncated(&mut self, s: &str, max_width: u16) -> io::Result<()> {
        let max = max_width as usize;
        if s.len() <= max {
            write!(self.out, "{}", s)
        } else {
            // Truncate at char boundary
            let mut end = max;
            while end > 0 && !s.is_char_boundary(end) {
                end -= 1;
            }
            write!(self.out, "{}", &s[..end])
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        if self.mouse {
            // Disable mouse tracking
            let _ = write!(self.out, "\x1b[?1000l\x1b[?1006l");
        }
        // Show cursor, exit alternate screen
        let _ = write!(self.out, "\x1b[?25h\x1b[?1049l");
        let _ = self.out.flush();

        // Restore original terminal settings
        let stdin_fd = io::stdin().as_raw_fd();
        unsafe {
            libc::tcsetattr(stdin_fd, libc::TCSAFLUSH, &self.original_termios);
        }
    }
}

fn get_window_size() -> io::Result<(u16, u16)> {
    #[repr(C)]
    struct WinSize {
        ws_row: u16,
        ws_col: u16,
        ws_xpixel: u16,
        ws_ypixel: u16,
    }

    let mut ws: WinSize = unsafe { std::mem::zeroed() };
    let fd = io::stdout().as_raw_fd();

    if unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) } == -1 {
        return Err(io::Error::last_os_error());
    }

    Ok((ws.ws_row, ws.ws_col))
}
