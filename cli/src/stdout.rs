use std::io::{self, Write};

/// Run `render` against a locked stdout, treating a closed pipe as a clean stop.
///
/// `println!` panics once a downstream reader exits, which `… | head` does by
/// design. The panic surfaces only when the output outgrows the pipe buffer, so
/// a tool that prints through the macros looks correct on small inputs and fails
/// on large ones.
///
/// The flush is inside the guard because a buffered write can succeed and the
/// flush behind it still meet the closed pipe.
pub fn with_stdout<F>(render: F) -> io::Result<()>
where
    F: FnOnce(&mut io::StdoutLock<'_>) -> io::Result<()>,
{
    let mut out = io::stdout().lock();
    match render(&mut out).and_then(|()| out.flush()) {
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        other => other,
    }
}
