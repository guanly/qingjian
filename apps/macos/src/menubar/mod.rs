//! 菜单栏：输入法菜单。
//!
//! 菜单是一份 NSMenu，挂在系统输入源菜单上（IMK `menu` 回调）——输入法激活时点菜单栏的输入法图标就能看到。
//! 不再另挂「中 / 英」状态项：系统的输入源图标是静态的（`tsInputMethodIconFileKey` 只在安装时读一次），
//! 多挂一个只会让菜单栏出现两个图标；当前中英由右上角的 OSD 提示（`ModeOsd`）交代。
//!
//! 菜单只是配置文件的运行时前端：每个开关点下去先改 `config.toml` 再热加载，不另存一套状态；
//! 手改配置文件与点菜单走的是同一条 [`crate::host::Host::apply_config`] 通路。
//!
//! 输入源菜单由 IMK 转发到控制器，sender 是带 `IMKCommandMenuItem` 键的字典，
//! 动作本身编码在菜单项的 tag 里（[`action_from_sender`] 认这一种形态）。

mod action;
mod menu;
mod target;

use objc2::runtime::AnyObject;
use objc2_app_kit::NSMenuItem;
use objc2_foundation::NSDictionary;
use objc2_input_method_kit::kIMKCommandMenuItemName;

pub use action::MenuAction;
pub use menu::InputMenu;

/// 从 `menuAction:` 的 sender 里认出被点的是哪一项。
pub fn action_from_sender(sender: Option<&AnyObject>) -> Option<MenuAction> {
    let sender = sender?;
    let tag = if let Some(item) = sender.downcast_ref::<NSMenuItem>() {
        item.tag()
    } else {
        let info = sender.downcast_ref::<NSDictionary<AnyObject, AnyObject>>()?;
        // SAFETY: 只读 IMK 导出的常量字符串
        let key: &AnyObject = unsafe { kIMKCommandMenuItemName };
        info.objectForKey(key)?.downcast::<NSMenuItem>().ok()?.tag()
    };
    MenuAction::from_tag(tag)
}
