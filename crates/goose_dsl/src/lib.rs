//! # Goose DSL: Zero-GC Stack Allocation & Mobile Touch Primitives
//!
//! Brings the core ergonomics of Wouter van Oortmerssen's Goose Systems
//! Programming Language (no heap, compiler-managed stack semantics, fast inline slices)
//! into idiomatic pure Rust for Lemuria and Kuroshio edge devices.

pub use glam::{DQuat, DVec3, Quat, Vec2, Vec3};

/// Stack-backed fixed capacity vector that behaves like Goose's inline arrays `T[..N]`
#[derive(Debug, Clone, PartialEq)]
pub struct StackArray<T, const N: usize> {
    data: [Option<T>; N],
    len: usize,
}

impl<T: Copy + Default, const N: usize> Default for StackArray<T, N> {
    fn default() -> Self {
        Self {
            data: [None; N],
            len: 0,
        }
    }
}

impl<T: Copy + Default, const N: usize> StackArray<T, N> {
    pub const fn new() -> Self {
        Self {
            data: [None; N],
            len: 0,
        }
    }

    pub fn push(&mut self, val: T) -> bool {
        if self.len < N {
            self.data[self.len] = Some(val);
            self.len += 1;
            true
        } else {
            false
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[Option<T>] {
        &self.data[..self.len]
    }
}

/// Mobile Touch Event Abstraction (for phones and compact touchscreen nodes)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TouchPhase {
    Down,
    Move,
    Up,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TouchEvent {
    pub x: f32,
    pub y: f32,
    pub phase: TouchPhase,
    pub timestamp_ms: u64,
}

/// Mobile Screen Orientation & Scale Profile
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenProfile {
    pub width: f32,
    pub height: f32,
    pub dpi_scale: f32,
    pub is_touchscreen: bool,
    pub is_portrait: bool,
}

impl ScreenProfile {
    pub fn phone_portrait(width: f32, height: f32, dpi: f32) -> Self {
        Self {
            width,
            height,
            dpi_scale: dpi,
            is_touchscreen: true,
            is_portrait: height > width,
        }
    }

    pub fn pi_touch_screen() -> Self {
        Self {
            width: 800.0,
            height: 480.0,
            dpi_scale: 1.0,
            is_touchscreen: true,
            is_portrait: false,
        }
    }

    pub fn desktop_1080p() -> Self {
        Self {
            width: 1920.0,
            height: 1080.0,
            dpi_scale: 1.0,
            is_touchscreen: false,
            is_portrait: false,
        }
    }
}

/// Macro mimicking Goose's `print(...)` without requiring complex formatting strings
#[macro_export]
macro_rules! goose_print {
    ($($arg:tt)*) => {
        println!($($arg)*);
    };
}
