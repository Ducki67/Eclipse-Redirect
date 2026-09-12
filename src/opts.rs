// Eclipse Redirect - configuration. The only file with comments.

#[derive(Clone, Copy, PartialEq)]
pub enum UrlSet {
    Default, // epic's domains, the normal private server setup
    Hybrid,  // only profile, version and content paths
    Dev,     // only profile and content paths
    All,     // every request
}

pub const BACKEND: &str = "http://127.0.0.1:3551"; // your backend url
pub const URL_SET: UrlSet = UrlSet::All;
pub const CONSOLE: bool = true; // create console window
pub const LOG_REQUESTS: bool = true; // log every url and rewrite, to the console and %TEMP%\eclipse_redirect.log
pub const B_HAS_PUSH_WIDGET: bool = false; // enable if gs closes a few seconds after it starts listening. breaks closing the client (don't use in a launcher build)

// misc options, don't change unless you know what you're doing
pub const B_USE_ARG_PARAMS: bool = false; // read -backend=http://IP:PORT from the command line, makes CONSOLE do nothing
pub const B_MANUAL_MAPPING: bool = false; // if you're using EAC & a manual mapper
pub const PROCESS_REQUEST_VTABLE_RVA: u64 = 0x0A793690; // 26.20 ProcessRequest vtable slot, prologue is checked before use. 0 = scan
pub const SET_URL_INDEX: i64 = 0; // 0 = detect at runtime
