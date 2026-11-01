//! Console messages: every line ref-web prints starts with the app name.

/// `println!` with the app name in front.
macro_rules! say {
    ($($arg:tt)*) => {
        println!("{}: {}", $crate::routes::APP_NAME, format_args!($($arg)*))
    };
}

/// `eprintln!` with the app name in front.
macro_rules! say_err {
    ($($arg:tt)*) => {
        eprintln!("{}: {}", $crate::routes::APP_NAME, format_args!($($arg)*))
    };
}
