// Macro for automatically quieting printlns

use log::LevelFilter;

pub static mut LOG_LEVEL: LevelFilter = LevelFilter::Info;

#[macro_export]
macro_rules! cli_message {
    () => {
        print!("\n")
    };
    ($($arg:tt)*) => {{
        let _is_not_quiet = unsafe {
            util::LOG_LEVEL != LevelFilter::Off
        };
        if _is_not_quiet {
            println!($($arg)*);
        }
    }};
}
