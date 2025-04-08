// Macro for automatically quieting printlns

use std::sync::RwLock;

use log::LevelFilter;

static _LOG_LEVEL: RwLock<LevelFilter> = RwLock::new(LevelFilter::Info);

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
