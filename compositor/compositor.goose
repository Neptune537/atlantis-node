// Kuroshio Lemuria-Nautilus 3D Spatial Compositor Engine
// Implemented in The Goose Systems Programming Language (aardappel.github.io/goose)
// Zero-GC Stack Allocation, Real-Time Dynamic 3D Spatial State Machine

struct SpatialVec3 {
    x: f64,
    y: f64,
    z: f64
}

struct SpatialQuat {
    x: f64,
    y: f64,
    z: f64,
    w: f64
}

struct NautilusGlobeState {
    spin_angle_deg: f64,
    idle_spin_rate: f64,
    camera_distance: f64,
    origin_ecef: SpatialVec3
}

struct MenuOverlayState {
    is_active: bool,
    break_apart_progress: f64, // 0.0 = fully centered menu, 1.0 = broken apart to sides
    left_wing_offset_x: f64,
    right_wing_offset_x: f64,
    hud_opacity: f64
}

struct LemuriaCompositor {
    frame_index: i64,
    fps_target: i32,
    mode: u8[..32], // "IDLE_MENU", "TRANSITIONING", "GAMEPLAY_GLOBE"
    globe: NautilusGlobeState,
    menu: MenuOverlayState
}

fn init_compositor() -> LemuriaCompositor {
    var g = NautilusGlobeState {
        spin_angle_deg: 0.0,
        idle_spin_rate: 0.25, // slow majestic idle spin
        camera_distance: 6378137.0 + 400000.0, // WGS84 Earth radius + 400km orbit
        origin_ecef: SpatialVec3 { x: 35.6586, y: 139.7454, z: 15.0 } // Tokyo / Kuroshio Trench Anchor
    };

    var m = MenuOverlayState {
        is_active: true,
        break_apart_progress: 0.0,
        left_wing_offset_x: 0.0,
        right_wing_offset_x: 0.0,
        hud_opacity: 1.0
    };

    return LemuriaCompositor {
        frame_index: 0,
        fps_target: 60,
        mode: "IDLE_MENU",
        globe: g,
        menu: m
    };
}

fn step_compositor(comp: LemuriaCompositor&, dt: f64, request_gameplay: bool) {
    comp.frame_index++;

    // 1. Globe slow spinning rotation in the background
    comp.globe.spin_angle_deg += comp.globe.idle_spin_rate * dt * 60.0;
    if comp.globe.spin_angle_deg >= 360.0 {
        comp.globe.spin_angle_deg -= 360.0;
    }

    // 2. Mutual Spawning & Break-Apart Animation State Machine
    if request_gameplay {
        // Break apart menu panels to the left and right, revealing the globe / gameplay
        if comp.menu.break_apart_progress < 1.0 {
            comp.menu.break_apart_progress += dt * 2.0; // 0.5s transition
            if comp.menu.break_apart_progress > 1.0 { comp.menu.break_apart_progress = 1.0; }
            comp.mode = "TRANSITIONING";
        } else {
            comp.mode = "GAMEPLAY_GLOBE";
            comp.menu.is_active = false;
        }
    } else {
        // Return to Idle Menu mode: Panels snap back from the sides, globe idles in background
        if comp.menu.break_apart_progress > 0.0 {
            comp.menu.break_apart_progress -= dt * 2.0;
            if comp.menu.break_apart_progress < 0.0 { comp.menu.break_apart_progress = 0.0; }
            comp.mode = "TRANSITIONING";
        } else {
            comp.mode = "IDLE_MENU";
            comp.menu.is_active = true;
        }
    }

    // Wing offsets in virtual screen pixels (e.g. 1920x1080)
    comp.menu.left_wing_offset_x = -comp.menu.break_apart_progress * 960.0;
    comp.menu.right_wing_offset_x = comp.menu.break_apart_progress * 960.0;
    comp.menu.hud_opacity = 1.0 - comp.menu.break_apart_progress;
}

fn print_compositor_telemetry(c: LemuriaCompositor) {
    print("--------------------------------------------------------------------------------");
    print("  LEMURIA COMPOSITOR [FRAME #", c.frame_index, "] - MODE: ", c.mode);
    print("  Nautilus Globe Idle Angle : ", c.globe.spin_angle_deg, " deg");
    print("  Menu Break-Apart Progress : ", c.menu.break_apart_progress * 100.0, " %");
    print("  Left Wing Translation     : ", c.menu.left_wing_offset_x, " px");
    print("  Right Wing Translation    : ", c.menu.right_wing_offset_x, " px");
    print("  HUD Overlay Opacity       : ", c.menu.hud_opacity);
    print("--------------------------------------------------------------------------------");
}

fn main() {
    print("================================================================================");
    print("  KUROSHIO GYRE :: LEMURIA 3D SPATIAL COMPOSITOR (GOOSE ZERO-GC RUNTIME)");
    print("  Features: Co-spawning Nautilus/Lite + Menu Break-Apart + Background 3D Globe");
    print("================================================================================");

    var comp = init_compositor();

    print("\n[PHASE 1] Menu Mode: Nautilus 3D Globe idling in slow background rotation...");
    for i in 5 {
        step_compositor(comp, 0.0166, false);
    }
    print_compositor_telemetry(comp);

    print("\n[PHASE 2] User Trigger: Menu breaks apart to sides, revealing globe / gameplay...");
    for i in 30 {
        step_compositor(comp, 0.0166, true);
    }
    print_compositor_telemetry(comp);

    print("\n[PHASE 3] In-Gameplay: Menu hidden / pushed to wings, globe fully active...");
    for i in 5 {
        step_compositor(comp, 0.0166, true);
    }
    print_compositor_telemetry(comp);

    print("\n[PHASE 4] Return to Menu: Panels slide back from sides over idling globe...");
    for i in 35 {
        step_compositor(comp, 0.0166, false);
    }
    print_compositor_telemetry(comp);
}
