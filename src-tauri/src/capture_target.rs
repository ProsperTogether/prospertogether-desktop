use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode")]
pub enum CaptureTarget {
    #[serde(rename = "screen")]
    Screen,
    #[serde(rename = "monitor")]
    Monitor { x: i32, y: i32, width: u32, height: u32 },
    #[serde(rename = "window")]
    Window { title: String, hwnd: isize, rect: Rect },
    #[serde(rename = "region")]
    Region { x: i32, y: i32, width: u32, height: u32 },
}
