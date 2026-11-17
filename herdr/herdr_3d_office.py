#!/usr/bin/env python3
"""
Herdr 3D Spatial Office Floor
A live 3D visual office floor where every AI employee/daemon (Triton, Ursula, Sebastian, Herdr Scanner)
is a physical character you can watch, select, inspect, and speak with.
Runs on Port 8899.
"""

import json
import time
from http.server import HTTPServer, BaseHTTPRequestHandler

PORT = 8899

# Live State of the Team 7 Triad & Herdr Workforce
WORKFORCE_STATE = {
    "triton": {
        "id": "triton",
        "name": "Triton",
        "role": "Chief of Staff & Tier 1 Verifier",
        "team": "EXECUTIVE & CORE CONSENSUS",
        "room": "executive_suite",
        "status": "working", # "working", "awaiting_input", "available", "error"
        "activity": "Verifying OpenRaft block #144002 and node health",
        "desk_pos": [-6.0, 0.0, -4.0],
        "speech_bubble": "Auditing Block #144002",
        "node_ip": "10.0.0.58"
    },
    "ursula": {
        "id": "ursula",
        "name": "Ursula",
        "role": "Growth Strategist & Harness Improver",
        "team": "STRATEGY & MONETIZATION",
        "room": "trading_floor",
        "status": "working",
        "activity": "Evaluating SOL/USDC VWAP consolidation & SBIR Phase I drafts",
        "desk_pos": [6.0, 0.0, -4.0],
        "speech_bubble": "SOL/USDC VWAP 72.97 | Hold Conviction 6.5",
        "node_ip": "Host Dual-GPU"
    },
    "sebastian": {
        "id": "sebastian",
        "name": "Sebastian",
        "role": "Marketing, Sales & Edge Sentinel",
        "team": "GROWTH & OUTREACH",
        "room": "media_lounge",
        "status": "working",
        "activity": "Streaming Siren Radio 24/7 & syndicating VisionQuest on TR7N7T7",
        "desk_pos": [-6.0, 0.0, 4.0],
        "speech_bubble": "Siren Radio Live | 0 Dropouts",
        "node_ip": "10.0.0.60"
    },
    "herdr_arb": {
        "id": "herdr_arb",
        "name": "Herdr Arb Bot",
        "role": "Jupiter DEX Liquidity Sentinel",
        "team": "STRATEGY & MONETIZATION",
        "room": "trading_floor",
        "status": "available",
        "activity": "Scanning Solana DEX triangular spread routes",
        "desk_pos": [6.0, 0.0, 4.0],
        "speech_bubble": "Mempool Spread: 0.12% (Below Threshold)",
        "node_ip": "10.0.0.58"
    }
}

HTML_OFFICE = """<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Herdr 3D Spatial Office Floor • Kuroshio Gyre</title>
    <script src="https://cdnjs.cloudflare.com/ajax/libs/three.js/r128/three.min.js"></script>
    <style>
        :root {
            --bg: #030712;
            --surface: rgba(15, 23, 42, 0.85);
            --border: #1e293b;
            --accent: #38bdf8;
            --accent-glow: rgba(56, 189, 248, 0.3);
            --gold: #fbbf24;
            --green: #22c55e;
            --red: #ef4444;
            --text: #f8fafc;
            --muted: #94a3b8;
        }
        * { box-sizing: border-box; margin: 0; padding: 0; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, monospace; }
        body { background: var(--bg); color: var(--text); overflow: hidden; width: 100vw; height: 100vh; }
        #canvas3d { width: 100%; height: 100%; display: block; }

        /* Top HUD Bar */
        .hud-top {
            position: absolute;
            top: 20px;
            left: 24px;
            right: 24px;
            display: flex;
            justify-content: space-between;
            align-items: center;
            pointer-events: none;
            z-index: 10;
        }
        .brand-pill {
            background: var(--surface);
            backdrop-filter: blur(12px);
            border: 1px solid var(--border);
            border-radius: 12px;
            padding: 10px 18px;
            display: flex;
            align-items: center;
            gap: 12px;
            pointer-events: auto;
            box-shadow: 0 10px 30px rgba(0,0,0,0.5);
        }
        .brand-logo {
            width: 32px;
            height: 32px;
            background: linear-gradient(135deg, #0284c7, #38bdf8);
            border-radius: 8px;
            display: flex;
            align-items: center;
            justify-content: center;
            font-weight: 800;
            color: #fff;
        }
        .brand-text h1 { font-size: 1rem; font-weight: 700; letter-spacing: 0.05em; }
        .brand-text p { font-size: 0.7rem; color: var(--muted); text-transform: uppercase; }

        .controls-pill {
            background: var(--surface);
            backdrop-filter: blur(12px);
            border: 1px solid var(--border);
            border-radius: 12px;
            padding: 8px 16px;
            display: flex;
            gap: 12px;
            pointer-events: auto;
        }
        .btn-tool {
            background: transparent;
            border: 1px solid var(--border);
            color: var(--text);
            padding: 6px 12px;
            border-radius: 6px;
            font-size: 0.75rem;
            cursor: pointer;
            transition: 0.2s;
        }
        .btn-tool:hover, .btn-tool.active {
            background: var(--accent);
            color: #020617;
            font-weight: 700;
            border-color: var(--accent);
        }

        /* Character Inspection Working Card */
        .character-card {
            position: absolute;
            right: 32px;
            bottom: 32px;
            width: 380px;
            background: var(--surface);
            backdrop-filter: blur(16px);
            border: 1px solid var(--border);
            border-radius: 16px;
            padding: 24px;
            display: none;
            flex-direction: column;
            gap: 16px;
            box-shadow: 0 25px 50px rgba(0,0,0,0.7);
            z-index: 20;
            animation: slideUp 0.3s cubic-bezier(0.16, 1, 0.3, 1);
        }
        @keyframes slideUp { from { transform: translateY(20px); opacity: 0; } to { transform: translateY(0); opacity: 1; } }

        .card-header { display: flex; justify-content: space-between; align-items: flex-start; }
        .char-avatar { width: 48px; height: 48px; border-radius: 10px; background: #1e293b; display: flex; align-items: center; justify-content: center; font-size: 1.5rem; }
        .char-info h2 { font-size: 1.15rem; font-weight: 700; }
        .char-info p { font-size: 0.75rem; color: var(--accent); text-transform: uppercase; font-weight: 600; margin-top: 2px; }
        .close-btn { background: transparent; border: none; color: var(--muted); font-size: 1.2rem; cursor: pointer; }
        .close-btn:hover { color: #fff; }

        .status-badge { display: inline-flex; align-items: center; gap: 6px; padding: 4px 10px; border-radius: 9999px; font-size: 0.75rem; font-weight: 700; text-transform: uppercase; background: rgba(34, 197, 94, 0.15); border: 1px solid var(--green); color: var(--green); }
        .pulse-dot { width: 6px; height: 6px; background: currentColor; border-radius: 50%; box-shadow: 0 0 6px currentColor; }

        .activity-box { background: rgba(2, 6, 23, 0.7); border: 1px solid var(--border); border-radius: 8px; padding: 12px; font-size: 0.8rem; line-height: 1.4; color: #cbd5e1; }
        .activity-label { font-size: 0.65rem; color: var(--muted); text-transform: uppercase; font-weight: 700; margin-bottom: 4px; }

        .msg-input { width: 100%; background: #020617; border: 1px solid var(--border); border-radius: 8px; padding: 10px 14px; color: #fff; font-size: 0.85rem; outline: none; }
        .msg-input:focus { border-color: var(--accent); }
        .send-btn { background: #0284c7; color: #fff; border: none; border-radius: 8px; padding: 10px; font-weight: 700; font-size: 0.85rem; cursor: pointer; transition: 0.2s; }
        .send-btn:hover { background: var(--accent); color: #020617; }

        /* Floating Overheads in 2D Screen Space */
        .overhead-label {
            position: absolute;
            transform: translate(-50%, -100%);
            pointer-events: none;
            display: flex;
            flex-direction: column;
            align-items: center;
            gap: 4px;
            transition: 0.1s ease-out;
            z-index: 5;
        }
        .speech-bubble {
            background: rgba(15, 23, 42, 0.95);
            border: 1px solid var(--accent);
            color: #fff;
            padding: 4px 10px;
            border-radius: 6px;
            font-size: 0.7rem;
            white-space: nowrap;
            box-shadow: 0 4px 12px rgba(0,0,0,0.5);
        }
        .name-tag {
            font-size: 0.75rem;
            font-weight: 700;
            color: var(--text);
            text-shadow: 0 2px 4px rgba(0,0,0,0.8);
            display: flex;
            align-items: center;
            gap: 6px;
        }
    </style>
</head>
<body>
    <div class="hud-top">
        <div class="brand-pill">
            <div class="brand-logo">H7</div>
            <div class="brand-text">
                <h1>HERDR 3D SPATIAL OFFICE FLOOR</h1>
                <p>Team 7 Triad & Autonomous Workforce</p>
            </div>
        </div>

        <div class="controls-pill">
            <button class="btn-tool active" onclick="resetCamera()">Reset View</button>
            <button class="btn-tool" id="tourBtn" onclick="toggleTour()">Cinematic Tour</button>
            <button class="btn-tool" onclick="shareClip()">Share Clip</button>
        </div>
    </div>

    <div id="labelsContainer"></div>

    <!-- Live Working Card on Select -->
    <div class="character-card" id="charCard">
        <div class="card-header">
            <div style="display:flex; gap:12px; align-items:center;">
                <div class="char-avatar" id="cAvatar">🔱</div>
                <div class="char-info">
                    <h2 id="cName">Triton</h2>
                    <p id="cRole">Chief of Staff</p>
                </div>
            </div>
            <button class="close-btn" onclick="closeCard()">&times;</button>
        </div>

        <div style="display:flex; justify-content:space-between; align-items:center;">
            <div class="status-badge" id="cStatusBadge"><span class="pulse-dot"></span> <span id="cStatusText">Working</span></div>
            <span style="font-size:0.75rem; color:var(--muted);" id="cNodeIp">10.0.0.58</span>
        </div>

        <div class="activity-box">
            <div class="activity-label">Live Active Activity</div>
            <div id="cActivity">Verifying OpenRaft block #144002 and node health</div>
        </div>

        <div style="display:flex; flex-direction:column; gap:8px;">
            <input type="text" class="msg-input" id="cMsgInput" placeholder="Direct instruction...">
            <button class="send-btn" onclick="sendInstruction()">Dispatch Directive</button>
        </div>
    </div>

    <div id="canvas3d"></div>

    <script>
        let scene, camera, renderer, raycaster, mouse;
        let employees = {};
        let selectedEmpId = null;
        let isTouring = false;
        let tourAngle = 0;
        let previousCameraPos = new THREE.Vector3(0, 18, 22);

        const WORKFORCE = """ + json.dumps(WORKFORCE_STATE) + """;

        function init() {
            const container = document.getElementById("canvas3d");
            scene = new THREE.Scene();
            scene.background = new THREE.Color(0x030712);
            scene.fog = new THREE.FogExp2(0x030712, 0.02);

            camera = new THREE.PerspectiveCamera(45, window.innerWidth / window.innerHeight, 0.1, 1000);
            camera.position.set(0, 18, 22);
            camera.lookAt(0, 0, 0);

            renderer = new THREE.WebGLRenderer({ antialias: true });
            renderer.setSize(window.innerWidth, window.innerHeight);
            renderer.shadowMap.enabled = true;
            container.appendChild(renderer.domElement);

            raycaster = new THREE.Raycaster();
            mouse = new THREE.Vector2();

            // Lighting
            const ambient = new THREE.AmbientLight(0xffffff, 0.6);
            scene.add(ambient);

            const dirLight = new THREE.DirectionalLight(0x38bdf8, 1.2);
            dirLight.position.set(10, 20, 10);
            dirLight.castShadow = true;
            scene.add(dirLight);

            // Floor & Rooms
            buildOfficeFloor();

            // Spawn Character Totems
            spawnCharacters();

            window.addEventListener('resize', onWindowResize);
            window.addEventListener('pointerdown', onPointerDown);

            animate();
        }

        function buildOfficeFloor() {
            // Main Floor Grid
            const floorGeo = new THREE.PlaneGeometry(32, 24);
            const floorMat = new THREE.MeshStandardMaterial({ color: 0x0a0f1d, roughness: 0.8 });
            const floor = new THREE.Mesh(floorGeo, floorMat);
            floor.rotation.x = -Math.PI / 2;
            floor.receiveShadow = true;
            scene.add(floor);

            const grid = new THREE.GridHelper(32, 32, 0x1e293b, 0x0f172a);
            grid.position.y = 0.01;
            scene.add(grid);

            // Glass Divider Walls
            const wallMat = new THREE.MeshPhysicalMaterial({ color: 0x38bdf8, transparent: true, opacity: 0.15, roughness: 0.1, transmission: 0.9, thickness: 0.5 });
            
            // Middle dividing wall
            const wall1 = new THREE.Mesh(new THREE.BoxGeometry(0.2, 3, 24), wallMat);
            wall1.position.set(0, 1.5, 0);
            scene.add(wall1);

            // Horizontal dividers
            const wall2 = new THREE.Mesh(new THREE.BoxGeometry(32, 3, 0.2), wallMat);
            wall2.position.set(0, 1.5, 0);
            scene.add(wall2);
        }

        function spawnCharacters() {
            const colors = {
                triton: 0x38bdf8,     // Cyan / Royal Blue
                ursula: 0xa855f7,     // Purple / Mystical
                sebastian: 0xef4444,  // Red / Coral Edge
                herdr_arb: 0x22c55e   // Green / Yield
            };

            for(let id in WORKFORCE) {
                const emp = WORKFORCE[id];
                const group = new THREE.Group();
                group.position.set(emp.desk_pos[0], 0, emp.desk_pos[2]);

                // Desk
                const deskGeo = new THREE.BoxGeometry(2.4, 0.9, 1.4);
                const deskMat = new THREE.MeshStandardMaterial({ color: 0x1e293b });
                const desk = new THREE.Mesh(deskGeo, deskMat);
                desk.position.set(0, 0.45, 0);
                group.add(desk);

                // Monitor
                const monGeo = new THREE.BoxGeometry(1.2, 0.6, 0.05);
                const monMat = new THREE.MeshBasicMaterial({ color: 0x38bdf8 });
                const mon = new THREE.Mesh(monGeo, monMat);
                mon.position.set(0, 1.15, 0.2);
                group.add(mon);

                // Character Bot / Hologram
                const bodyGeo = new THREE.CylinderGeometry(0.35, 0.35, 1.2, 16);
                const bodyMat = new THREE.MeshStandardMaterial({ color: colors[id] || 0xffffff, roughness: 0.3 });
                const body = new THREE.Mesh(bodyGeo, bodyMat);
                body.position.set(0, 0.6, -0.9);
                body.userData = { empId: id };
                group.add(body);

                // Head with Status Antenna
                const headGeo = new THREE.SphereGeometry(0.3, 16, 16);
                const headMat = new THREE.MeshStandardMaterial({ color: 0xffffff });
                const head = new THREE.Mesh(headGeo, headMat);
                head.position.set(0, 1.45, -0.9);
                head.userData = { empId: id };
                group.add(head);

                // Pulsing Status Halo Ring
                const ringGeo = new THREE.RingGeometry(0.6, 0.75, 32);
                const ringMat = new THREE.MeshBasicMaterial({ color: colors[id], side: THREE.DoubleSide });
                const ring = new THREE.Mesh(ringGeo, ringMat);
                ring.rotation.x = -Math.PI / 2;
                ring.position.set(0, 0.05, -0.9);
                group.add(ring);

                scene.add(group);
                employees[id] = { group, body, ring, data: emp };
            }
        }

        function onPointerDown(event) {
            if(event.target.tagName !== "CANVAS") return;
            mouse.x = (event.clientX / window.innerWidth) * 2 - 1;
            mouse.y = -(event.clientY / window.innerHeight) * 2 + 1;

            raycaster.setFromCamera(mouse, camera);
            const intersects = raycaster.intersectObjects(scene.children, true);

            for (let i = 0; i < intersects.length; i++) {
                const hit = intersects[i].object;
                if(hit.userData && hit.userData.empId) {
                    selectEmployee(hit.userData.empId);
                    break;
                }
            }
        }

        function selectEmployee(empId) {
            selectedEmpId = empId;
            const emp = WORKFORCE[empId];

            // Open working card
            document.getElementById("cName").innerText = emp.name;
            document.getElementById("cRole").innerText = emp.role;
            document.getElementById("cActivity").innerText = emp.activity;
            document.getElementById("cNodeIp").innerText = emp.node_ip;
            document.getElementById("charCard").style.display = "flex";

            // Camera Fly-In conversation shot
            const targetPos = new THREE.Vector3(emp.desk_pos[0], 3, emp.desk_pos[2] + 4);
            flyCameraTo(targetPos, new THREE.Vector3(emp.desk_pos[0], 1, emp.desk_pos[2]));
        }

        function closeCard() {
            document.getElementById("charCard").style.display = "none";
            selectedEmpId = null;
            flyCameraTo(previousCameraPos, new THREE.Vector3(0, 0, 0));
        }

        function flyCameraTo(targetPos, lookAtTarget) {
            camera.position.copy(targetPos);
            camera.lookAt(lookAtTarget);
        }

        function resetCamera() {
            closeCard();
            isTouring = false;
            document.getElementById("tourBtn").classList.remove("active");
            camera.position.set(0, 18, 22);
            camera.lookAt(0, 0, 0);
        }

        function toggleTour() {
            isTouring = !isTouring;
            document.getElementById("tourBtn").classList.toggle("active", isTouring);
        }

        function shareClip() {
            alert("Capturing 5s telemetry clip of Kuroshio 3D Workforce...");
        }

        function sendInstruction() {
            const input = document.getElementById("cMsgInput");
            if(!input.value.trim()) return;
            alert(`Directive dispatched to ${WORKFORCE[selectedEmpId].name}: "${input.value}"`);
            input.value = "";
        }

        function onWindowResize() {
            camera.aspect = window.innerWidth / window.innerHeight;
            camera.updateProjectionMatrix();
            renderer.setSize(window.innerWidth, window.innerHeight);
        }

        function animate() {
            requestAnimationFrame(animate);

            // Autonomous Idle Thinking Animations
            const time = Date.now() * 0.003;
            for(let id in employees) {
                const emp = employees[id];
                emp.ring.scale.setScalar(1.0 + Math.sin(time + emp.data.desk_pos[0]) * 0.1);
            }

            // Cinematic Self-Driving Tour
            if(isTouring) {
                tourAngle += 0.005;
                camera.position.x = Math.sin(tourAngle) * 20;
                camera.position.z = Math.cos(tourAngle) * 20;
                camera.position.y = 12 + Math.sin(tourAngle * 2) * 3;
                camera.lookAt(0, 1, 0);
            }

            renderer.render(scene, camera);
        }

        window.onload = init;
    </script>
</body>
</html>
"""

class OfficeHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path in ["/", "/index.html"]:
            self.send_response(200)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(HTML_OFFICE.encode("utf-8"))))
            self.end_headers()
            self.wfile.write(HTML_OFFICE.encode("utf-8"))
            return
        elif self.path == "/api/workforce":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps(WORKFORCE_STATE).encode("utf-8"))
            return
        self.send_response(404)
        self.end_headers()

if __name__ == "__main__":
    server = HTTPServer(("0.0.0.0", PORT), OfficeHandler)
    print(f"[*] Herdr 3D Spatial Office Floor running on http://127.0.0.1:{PORT}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
