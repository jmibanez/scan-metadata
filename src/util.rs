// Macro for automatically quieting printlns

use log::LevelFilter;

use std::sync::RwLock;

#[cfg(test)]
#[ctor::ctor]
fn init_logger_for_test() {
    use simplelog::{ColorChoice, Config, TermLogger, TerminalMode};

    TermLogger::init(
        LevelFilter::Debug,
        Config::default(),
        TerminalMode::Stderr,
        ColorChoice::Auto,
    );
}

static _LOG_LEVEL: RwLock<LevelFilter> = if cfg!(test) {
    RwLock::new(LevelFilter::Debug)
} else {
    RwLock::new(LevelFilter::Info)
};

pub fn set_log_level(level: LevelFilter) {
    *(_LOG_LEVEL.write().unwrap()) = level;
}

pub fn get_log_level() -> LevelFilter {
    *(_LOG_LEVEL.read().unwrap())
}

pub fn is_log_level(level: LevelFilter) -> bool {
    get_log_level() == level
}

pub fn is_not_quiet() -> bool {
    !is_log_level(LevelFilter::Off)
}

#[macro_export]
macro_rules! cli_message {
    () => {
        print!("\n")
    };
    ($($arg:tt)*) => {{
        if util::is_not_quiet() {
            println!($($arg)*);
        }
    }};
}
