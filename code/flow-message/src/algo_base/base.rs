// Adapted from MegFlow 95f870bf, flow-message/src/algo_base/base.rs.
// Copyright (c) 2019-2021 Megvii Inc. Apache-2.0.
// Rect 与 TrackState 保留原版 Rust 实现；其余算法消息仍待迁移。

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
    pub score: Option<f32>,
}

impl Rect {
    pub fn width(&self) -> f32 {
        self.x2 - self.x1
    }
    pub fn height(&self) -> f32 {
        self.y2 - self.y1
    }
    pub fn area(&self) -> f32 {
        self.width() * self.height()
    }
    pub fn in_frame(&self, frame_width: f32, frame_height: f32) -> bool {
        if self.x2 > frame_width || self.x1 < 0.0 || self.y2 > frame_height || self.y1 < 0.0 {
            return false;
        }
        true
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub enum TrackState {
    #[default]
    No = 0,
    New,
    Update,
    Miss,
    Die,
    Filtered,
    Select,
    Max,
}

impl TrackState {
    pub fn name(&self) -> &'static str {
        match self {
            Self::No => "no",
            Self::New => "new",
            Self::Update => "update",
            Self::Miss => "miss",
            Self::Die => "die",
            Self::Filtered => "filtered",
            Self::Select => "select",
            Self::Max => "max",
        }
    }
}

impl<'a> From<&'a str> for TrackState {
    fn from(value: &'a str) -> Self {
        match value {
            "no" => Self::No,
            "new" => Self::New,
            "update" => Self::Update,
            "miss" => Self::Miss,
            "die" => Self::Die,
            "filtered" => Self::Filtered,
            "select" => Self::Select,
            "max" => Self::Max,
            _ => unreachable!(),
        }
    }
}

#[allow(clippy::to_string_trait_impl)]
impl ToString for TrackState {
    fn to_string(&self) -> String {
        match self {
            TrackState::No => "no".to_string(),
            TrackState::New => "new".to_string(),
            TrackState::Update => "update".to_string(),
            TrackState::Miss => "miss".to_string(),
            TrackState::Die => "die".to_string(),
            TrackState::Filtered => "filtered".to_string(),
            TrackState::Select => "select".to_string(),
            TrackState::Max => "max".to_string(),
        }
    }
}

use std::ops::{Deref, DerefMut};

pub struct RecordVec<T>(pub(crate) Vec<T>, pub usize);

impl<T> RecordVec<T> {
    #[inline]
    pub fn push(&mut self, elem: T) {
        self.0.push(elem)
    }
}

impl<T> From<Vec<T>> for RecordVec<T> {
    fn from(value: Vec<T>) -> Self {
        let l = value.len();
        Self(value, l)
    }
}

impl<T> From<RecordVec<T>> for Vec<T> {
    fn from(value: RecordVec<T>) -> Self {
        value.0
    }
}

impl<T> Default for RecordVec<T> {
    fn default() -> Self {
        Self(vec![], 0)
    }
}

impl<T> Deref for RecordVec<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for RecordVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
