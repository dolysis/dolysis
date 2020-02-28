use {
    crate::{error::MainResult, prelude::*, ARGS},
    std::fmt,
    tracing_subscriber::{EnvFilter, FmtSubscriber},
};

pub mod tcp;

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
    fn span_print(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result;

    fn span_display(&self) -> LocalDisplay<Self>
    where
        Self: Sized,
    {
        LocalDisplay::new(self)
    }
}

pub struct LocalDisplay<'a, T> {
    owner: &'a T,
}

impl<'a, T> LocalDisplay<'a, T> {
    pub fn new(owner: &'a T) -> Self
    where
        T: SpanDisplay,
    {
        Self { owner }
    }
}

impl<'a, T> fmt::Display for LocalDisplay<'a, T>
where
    T: SpanDisplay,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.owner.span_print(f)
    }
}
