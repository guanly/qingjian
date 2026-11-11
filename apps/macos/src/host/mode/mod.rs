//! 中 / 英切换：单击 Shift 的边沿检测，按应用记住的模式，与切换时右上角的提示。

mod osd;
mod remembered;
mod switch;

pub use osd::ModeOsd;
pub use remembered::AppModes;
pub use switch::ModeSwitch;
