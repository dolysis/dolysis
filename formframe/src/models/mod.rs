use {
    crate::{error::MainResult, prelude::*, ARGS},
    std::fmt,
    tracing_subscriber::{EnvFilter, FmtSubscriber},
};

/// Initialize the global logger. This function must be called before ARGS is initialized,
/// otherwise logs generated during CLI parsing will be silently ignored
pub fn init_logging() {
    let root_subscriber = FmtSubscriber::builder()
        .with_writer(std::io::stderr)
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::default().add_directive(tracing_subscriber::filter::LevelFilter::INFO.into())
        }))
        .with_filter_reloading()
        .finish();
    tracing::subscriber::set_global_default(root_subscriber).expect("Failed to init logging");
    info!("<== Logs Start ==>")
}

/// This function should be the first to deref ARGS,
/// giving the program a chance to bail if anything went wrong on initialization.
/// It is an invariant of this program that any call to ARGs after this call will never fail
pub fn check_args() -> MainResult<()> {
    let args = ARGS.as_ref();
    match args {
        Ok(_) => Ok(()),
        Err(e) => {
            let e = Err(e.into());
            e
        }
    }
}

pub trait SpanDisplay {
    fn span_output(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;

    fn span_write<'a, F>(&'a self, f: &'a F) -> LocalDisplay<Self>
    where
        Self: Sized,
        F: Fn(&Self, &mut fmt::Formatter<'_>) -> fmt::Result,
    {
        LocalDisplay::new(self, f)
    }

    fn span_display<'a>(&'a self) -> LocalDisplay<Self>
    where
        Self: Sized,
    {
        self.span_write(&<Self as SpanDisplay>::span_output)
    }
}

pub struct LocalDisplay<'a, T> {
    owner: &'a T,
    display: &'a dyn Fn(&'a T, &mut fmt::Formatter<'_>) -> fmt::Result,
}

impl<'a, T> LocalDisplay<'a, T> {
    pub fn new<F>(owner: &'a T, args: &'a F) -> Self
    where
        F: Fn(&'a T, &mut fmt::Formatter<'_>) -> fmt::Result,
    {
        Self {
            owner,
            display: args,
        }
    }

    // pub fn as_display(&'a self) -> impl fmt::Display + 'a {
    //     self
    // }
}

impl<'a, T> fmt::Display for LocalDisplay<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let output = self.display;
        output(self.owner, f)
    }
}
