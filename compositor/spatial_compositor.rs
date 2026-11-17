//! # Nautilus Spatial 3D Compositor & Break-Apart Transition Engine
//!
//! Handles:
//! - Co-spawning between Lite (lightweight telemetry HUD) and Nautilus (planetary open world).
//! - Idle slow spinning rotation of the 3D globe in menu mode.
//! - Split-wing break-apart animation that slides HUD panels off to the sides
//!   to reveal the globe and active gameplay.

use glam::{DVec3, Vec3};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositorState {
    /// Idle menu overlay active, Nautilus globe spinning slowly in the background
    IdleMenu,
    /// Breaking apart or converging
    Transitioning,
    /// Fully revealed globe & gameplay mode, menus parked at wings or hidden
    GameplayGlobe,
}

#[derive(Debug, Clone)]
pub struct NautilusCompositorState {
    pub current_mode: CompositorState,
    pub spin_yaw_radians: f32,
    pub idle_spin_speed: f32,
    /// 0.0 = fully centered menu over globe, 1.0 = completely broken apart to the sides
    pub break_apart_pct: f32,
    pub camera_distance_m: f64,
    pub left_wing_offset_px: f32,
    pub right_wing_offset_px: f32,
    pub hud_alpha: f32,
}

impl Default for NautilusCompositorState {
    fn default() -> Self {
        Self {
            current_mode: CompositorState::IdleMenu,
            spin_yaw_radians: 0.0,
            idle_spin_speed: 0.05, // Slow majestic rotation
            break_apart_pct: 0.0,
            camera_distance_m: 6_378_137.0 + 850_000.0,
            left_wing_offset_px: 0.0,
            right_wing_offset_px: 0.0,
            hud_alpha: 1.0,
        }
    }
}

impl NautilusCompositorState {
    pub fn update(&mut self, dt_seconds: f32, request_gameplay: bool, screen_width_px: f32) {
        // 1. Idle Background Globe Rotation (continuous regardless of state)
        self.spin_yaw_radians += self.idle_spin_speed * dt_seconds;
        if self.spin_yaw_radians > std::f32::consts::TAU {
            self.spin_yaw_radians -= std::f32::consts::TAU;
        }

        // 2. Break-apart state transition
        let target_pct = if request_gameplay { 1.0 } else { 0.0 };
        let transition_speed = 2.5; // Fast responsive split

        if (self.break_apart_pct - target_pct).abs() > 0.001 {
            self.current_mode = CompositorState::Transitioning;
            if self.break_apart_pct < target_pct {
                self.break_apart_pct = (self.break_apart_pct + dt_seconds * transition_speed).min(1.0);
            } else {
                self.break_apart_pct = (self.break_apart_pct - dt_seconds * transition_speed).max(0.0);
            }
        } else {
            self.break_apart_pct = target_pct;
            self.current_mode = if target_pct >= 1.0 {
                CompositorState::GameplayGlobe
            } else {
                CompositorState::IdleMenu
            };
        }

        // 3. Dynamic split transforms
        let half_screen = screen_width_px * 0.55;
        self.left_wing_offset_px = -self.break_apart_pct * half_screen;
        self.right_wing_offset_px = self.break_apart_pct * half_screen;
        self.hud_alpha = (1.0 - self.break_apart_pct).clamp(0.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compositor_break_apart() {
        let mut comp = NautilusCompositorState::default();
        assert_eq!(comp.current_mode, CompositorState::IdleMenu);

        // Step forward requesting gameplay
        comp.update(0.5, true, 1920.0);
        assert_eq!(comp.current_mode, CompositorState::GameplayGlobe);
        assert!(comp.left_wing_offset_px < -500.0);
        assert!(comp.right_wing_offset_px > 500.0);
        assert_eq!(comp.hud_alpha, 0.0);
    }
}
