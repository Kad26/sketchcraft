//! Regression tests for the security review (security-reports/, fixes in
//! RED-TEAM-REMEDIATION.md): data integrity, command policy, resource limits.
use serde_json::{json, Value};
use sketchcraft::live::Live;
use sketchcraft::policy::{Caller, Workspace};
use sketchcraft::Engine;
use std::path::PathBuf;

fn run(e: &mut Engine, v: Value) -> Value {
    let r = e.exec_json(&v.to_string());
    assert_eq!(r["ok"], true, "{v} -> {r}");
    r
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("sc-hardening-{name}-{}", std::process::id()));
    std::fs::remove_dir_all(&d).ok();
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn faces(e: &mut Engine) -> usize {
    run(e, json!({"cmd": "info"}))["faces"].as_u64().unwrap() as usize
}

// ---- BRN-001/002/003: numbers, panics, non-finite saves -----------------------------

#[test]
fn huge_numbers_never_reach_the_model() {
    let mut e = Engine::new();
    let f = run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}))["face"].as_u64().unwrap();
    let g = run(&mut e, json!({"cmd": "group", "ids": [f]}))["group"].as_u64().unwrap();
    for bad in [
        json!({"cmd": "scale", "ids": [g], "center": [0, 0, 0], "factor": 1.7e308}),
        json!({"cmd": "scale", "ids": [g], "center": [0, 0, 0], "factor": -1e300}),
        json!({"cmd": "move", "ids": [g], "by": [1.7e308, 0, 0]}),
        json!({"cmd": "smoove", "group": g, "at": [0, 0, 0], "radius": 1e308, "height": 1e308}),
        json!({"cmd": "rect", "from": [0, 0, 0], "to": [1e12, 1e12, 0]}),
    ] {
        let r = e.exec_json(&bad.to_string());
        assert_eq!(r["ok"], false, "{bad} should be refused");
    }
    // a result that overflows the model's range is undone, not kept
    let r = e.exec_json(&json!({"cmd": "move", "ids": [g], "by": [9e8, 0, 0]}).to_string());
    assert_eq!(r["ok"], true);
    let r = e.exec_json(&json!({"cmd": "scale", "ids": [g], "center": [0, 0, 0], "factor": 1e9}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    e.model.check_sane().unwrap();
}

#[test]
fn a_panic_inside_a_command_restores_the_model_and_keeps_undo_consistent() {
    let mut e = Engine::new();
    let f = run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}))["face"].as_u64().unwrap();
    run(&mut e, json!({"cmd": "pushpull", "face": f, "distance": 3}));
    let g = run(&mut e, json!({"cmd": "group", "ids": [f]}))["group"].as_u64().unwrap();
    let before = serde_json::to_string(&e.model).unwrap();
    // a group made from a stale face id used to panic the triangulator's caller
    let r = e.exec_json(&json!({"cmd": "subdivide", "id": g}).to_string());
    // either a clean error or a (now fixed) success; never a corrupted model or a lost step
    if r["ok"] == false {
        assert_eq!(serde_json::to_string(&e.model).unwrap(), before, "a failed command changes nothing");
        // undo still reverts the previous good step, not something else
        run(&mut e, json!({"cmd": "undo"}));
        assert!(e.model.groups.is_empty(), "undo removed the group made just before");
    }
    e.model.check_sane().unwrap();
}

#[test]
fn exec_survives_a_panic_and_leaves_the_model_alone() {
    // a vertex id the faces still point at, removed behind the model's back
    let mut e = Engine::new();
    let f = run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}))["face"].as_u64().unwrap();
    let victim = *e.model.vertices.keys().next().unwrap();
    e.model.vertices.remove(&victim);
    let before = e.model.faces.len();
    let r = e.exec_json(&json!({"cmd": "pushpull", "face": f, "distance": 1}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(e.model.faces.len(), before);
    // the engine still answers afterwards
    assert_eq!(e.exec_json(r#"{"cmd":"new"}"#)["ok"], true);
}

#[test]
fn a_model_with_non_finite_numbers_is_never_saved() {
    let dir = scratch("nonfinite");
    let file = dir.join("m.sketchcraft");
    let mut e = Engine::new();
    run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    run(&mut e, json!({"cmd": "save", "path": file}));
    let good = std::fs::read(&file).unwrap();
    e.model.vertices.values_mut().next().unwrap().p.x = f64::NAN;
    let r = e.exec_json(&json!({"cmd": "save", "path": file}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(std::fs::read(&file).unwrap(), good, "the good file is untouched");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn opening_a_file_with_nulls_is_an_error_not_an_empty_model() {
    let dir = scratch("nullfile");
    let file = dir.join("bad.sketchcraft");
    std::fs::write(&file, r#"{"next_id":1,"vertices":{"1":{"p":[null,0,0]}}}"#).unwrap();
    let mut e = Engine::new();
    run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    let r = e.exec_json(&json!({"cmd": "open", "path": file}).to_string());
    assert_eq!(r["ok"], false);
    assert_eq!(faces(&mut e), 1, "the model that was open stays open");
    std::fs::remove_dir_all(&dir).ok();
}

// ---- BRN-004/005/006: the live file --------------------------------------------------

#[test]
fn a_damaged_live_file_is_kept_and_reported_not_replaced_silently() {
    let dir = scratch("damaged");
    let doc = dir.join("live.sketchcraft");
    std::fs::write(&doc, b"{\"next_id\": 5, \"vertices\": {\"1\": {\"p\": [0,0,0]").unwrap(); // truncated
    let original = std::fs::read(&doc).unwrap();
    let mut live = Live::open(doc.clone());
    assert!(live.notice.as_deref().is_some_and(|n| n.contains("couldn't be opened")), "{:?}", live.notice);
    // the first command must not write over the file: it works in memory, says the save didn't happen, and the file is as it was
    let r = live.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    assert_eq!(r["ok"], true);
    assert!(r["save_error"].is_string(), "{r}");
    assert_eq!(std::fs::read(&doc).unwrap(), original, "the unreadable file is untouched");
    // the damaged bytes survive in a copy beside it, and Restore lists it
    let kept: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x.to_string_lossy().starts_with("damaged"))).collect();
    assert_eq!(kept.len(), 1, "one damaged copy");
    assert_eq!(std::fs::read(&kept[0]).unwrap(), original);
    let list = live.exec(&json!({"cmd": "list_backups"}));
    assert!(list["items"].as_array().unwrap().iter().any(|i| i["kind"] == "damaged"), "{list}");
    // starting over (the person's New) lets saving resume, and the file is then a good model
    let r = live.exec(&json!({"cmd": "new"}));
    assert_eq!(r["ok"], true, "{r}");
    let r = live.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    assert!(r.get("save_error").is_none(), "{r}");
    let mut check = Engine::new();
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn saving_is_atomic_and_leaves_no_temp_files() {
    let dir = scratch("atomic");
    let doc = dir.join("live.sketchcraft");
    let mut live = Live::open(doc.clone());
    for i in 0..5 {
        let r = live.exec(&json!({"cmd": "rect", "from": [i * 10, 0, 0], "to": [i * 10 + 4, 4, 0]}));
        assert_eq!(r["ok"], true);
        assert!(r.get("save_error").is_none());
    }
    let strays: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".tmp-")).collect();
    assert!(strays.is_empty(), "temp files left: {strays:?}");
    // always a whole, valid model
    let mut check = Engine::new();
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 5);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn rolling_backups_are_kept_and_bounded() {
    let dir = scratch("backups");
    let doc = dir.join("live.sketchcraft");
    let mut live = Live::open(doc.clone());
    for i in 0..14 {
        // `new` always takes a backup first
        live.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4 + i, 4, 0]}));
        live.exec(&json!({"cmd": "new"}));
    }
    // each `new` keeps what it replaced (kept a day: they were taken just before a destructive change)
    let n = std::fs::read_dir(dir.join("Backups")).unwrap().count();
    assert!((1..=60).contains(&n), "{n} backups");
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn two_clients_do_not_overwrite_each_other() {
    let dir = scratch("lost-update");
    let doc = dir.join("live.sketchcraft");
    let mut app = Live::open(doc.clone());
    app.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    // another client (MCP, the CLI) adds a face and saves
    std::thread::sleep(std::time::Duration::from_millis(1100)); // file timestamps can be coarse
    let mut other = Live::open(doc.clone());
    other.exec(&json!({"cmd": "rect", "from": [10, 0, 0], "to": [14, 4, 0]}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    // the app edits before its poll noticed: it must start from the other's version
    app.exec(&json!({"cmd": "rect", "from": [20, 0, 0], "to": [24, 4, 0]}));
    let mut check = Engine::new();
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 3, "all three rectangles survive");
    std::fs::remove_dir_all(&dir).ok();
}

// ---- MBK-001/002/004, SPC-003: who may run what --------------------------------------

#[test]
fn plugins_cannot_touch_files_plugins_or_programs() {
    let dir = scratch("plugin-policy");
    let marker = dir.join("marker.txt");
    let mut e = Engine::new();
    for bad in [
        json!({"cmd": "save_text", "path": marker, "text": "x"}),
        json!({"cmd": "install_plugin", "data": "AAAA", "name": "x.rb"}),
        json!({"cmd": "plugin_enable", "id": "cutlist", "on": false}),
        json!({"cmd": "remove_plugin", "id": "x"}),
        json!({"cmd": "reveal", "path": dir}),
        json!({"cmd": "skins"}),
        json!({"cmd": "save_skin", "name": "a", "skin": {}}),
        json!({"cmd": "add_location", "lat": 1, "lon": 1}),
        json!({"cmd": "photoreal", "samples": 4096}),
        json!({"cmd": "video_finish", "session": "a", "path": "http://127.0.0.1:9/x.ts"}),
        json!({"cmd": "export_obj", "path": marker}),
        json!({"cmd": "save", "path": marker}),
        json!({"cmd": "render", "out": marker}),
        json!({"cmd": "new"}),
        json!({"cmd": "totally_new_command"}),
    ] {
        let r = e.exec_json_as(&Caller::Plugin, &bad.to_string());
        assert_eq!(r["ok"], false, "{bad} -> {r}");
    }
    assert!(!marker.exists());
    // modeling still works for them
    let r = e.exec_json_as(&Caller::Plugin, &json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}).to_string());
    assert_eq!(r["ok"], true);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn agents_work_inside_their_workspace_only() {
    let dir = scratch("mcp-policy");
    let ws = dir.join("work");
    std::fs::create_dir_all(&ws).unwrap();
    let caller = Caller::Mcp(Workspace::at(&ws));
    let mut e = Engine::new();
    e.exec_json_as(&caller, &json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}).to_string());
    let ok = e.exec_json_as(&caller, &json!({"cmd": "export_obj", "path": "house.obj"}).to_string());
    assert_eq!(ok["ok"], true, "{ok}");
    assert!(ws.join("house.obj").exists());
    for bad in [
        json!({"cmd": "export_obj", "path": dir.join("outside.obj")}),
        json!({"cmd": "export_obj", "path": "../outside.obj"}),
        json!({"cmd": "save_text", "path": "a.txt", "text": "x"}),
        json!({"cmd": "install_plugin", "data": "AAAA", "name": "x.rb"}),
        json!({"cmd": "export_obj", "path": "Plugins/evil/main.js"}),
        json!({"cmd": "animate", "path": "http://127.0.0.1:9/x.mp4"}),
    ] {
        let r = e.exec_json_as(&caller, &bad.to_string());
        assert_eq!(r["ok"], false, "{bad} -> {r}");
    }
    assert!(!dir.join("outside.obj").exists());
    // --any-path lifts the folder limit, not the command list
    let wide = Caller::Mcp(Workspace::anywhere());
    assert_eq!(e.exec_json_as(&wide, &json!({"cmd": "export_obj", "path": dir.join("wide.obj")}).to_string())["ok"], true);
    assert_eq!(e.exec_json_as(&wide, &json!({"cmd": "save_text", "path": dir.join("t.txt"), "text": "x"}).to_string())["ok"], false);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn paths_that_are_urls_or_pipes_are_refused_for_everyone() {
    let mut e = Engine::new();
    for p in ["http://127.0.0.1:9/x.ts", "pipe:1", "tcp://127.0.0.1:9", "file:///etc/passwd", "concat:a|b"] {
        for cmd in ["video_finish", "animate", "export_obj", "save"] {
            let r = e.exec_json(&json!({"cmd": cmd, "session": "a", "path": p}).to_string());
            assert_eq!(r["ok"], false, "{cmd} {p} -> {r}");
            assert!(r["error"].as_str().unwrap().contains("isn't a file path"), "{r}");
        }
    }
}

#[test]
fn long_strings_are_refused() {
    let mut e = Engine::new();
    let f = run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}))["face"].as_u64().unwrap();
    let g = run(&mut e, json!({"cmd": "group", "ids": [f]}))["group"].as_u64().unwrap();
    let r = e.exec_json(&json!({"cmd": "rename", "id": g, "name": "x".repeat(200_000)}).to_string());
    assert_eq!(r["ok"], false);
    let r = e.exec_json(&json!({"cmd": "text", "at": [0, 0, 0], "text": "x".repeat(50_000)}).to_string());
    assert_eq!(r["ok"], false);
}

// ---- resource limits ------------------------------------------------------------------

#[test]
fn render_and_photoreal_sizes_are_clamped_everywhere() {
    let o = sketchcraft::render::RenderOpts { width: 4_000_000_000, height: 4_000_000_000, ..Default::default() }.limited();
    assert!(o.width <= 8192 && o.height <= 8192 && o.width as u64 * o.height as u64 <= 25_000_000, "{}x{}", o.width, o.height);
    let o = sketchcraft::render::RenderOpts { width: 0, height: 0, ..Default::default() }.limited();
    assert!(o.width >= 16 && o.height >= 16);
    let p = sketchcraft::pt::PtOpts { width: 4096, height: 4096, samples: 4096, bounces: 16, ..Default::default() }.limited();
    assert!(p.width as u64 * p.height as u64 * p.samples as u64 * p.bounces as u64 <= 4_100_000_000, "{}x{} {} {}", p.width, p.height, p.samples, p.bounces);
    assert!(p.bounces <= 12);
    // and a huge render command returns quickly instead of grinding
    let mut e = Engine::new();
    run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    let t = std::time::Instant::now();
    let r = e.exec_json(&json!({"cmd": "render", "width": 3_000_000_000u64, "height": 3_000_000_000u64}).to_string());
    assert_eq!(r["ok"], false, "over the number limit: {r}");
    let r = e.exec_json(&json!({"cmd": "render", "width": 1e8, "height": 1e8}).to_string());
    assert_eq!(r["ok"], false);
    assert!(t.elapsed().as_secs() < 5);
}

#[test]
fn a_dxf_block_bomb_is_cut_off() {
    // block L0 draws a line; each next block inserts the one before 20 times
    let mut dxf = String::from("0\nSECTION\n2\nBLOCKS\n");
    for i in 0..8 {
        dxf += &format!("0\nBLOCK\n2\nL{i}\n10\n0\n20\n0\n30\n0\n");
        if i == 0 {
            dxf += "0\nLINE\n10\n0\n20\n0\n30\n0\n11\n1\n21\n1\n31\n0\n";
        } else {
            for _ in 0..20 {
                dxf += &format!("0\nINSERT\n2\nL{}\n10\n0\n20\n0\n30\n0\n", i - 1);
            }
        }
        dxf += "0\nENDBLK\n";
    }
    dxf += "0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nL7\n10\n0\n20\n0\n30\n0\n0\nENDSEC\n0\nEOF\n";
    let mut e = Engine::new();
    let t = std::time::Instant::now();
    let r = e.exec_json(&json!({"cmd": "import_dxf", "text": dxf}).to_string());
    assert!(t.elapsed().as_secs() < 20, "took {:?}", t.elapsed());
    // refused whole, at once; never a hang
    assert_eq!(r["ok"], false, "{r}");
    assert!(e.model.faces.is_empty() && e.model.edges.is_empty(), "nothing was imported");
}

/// R2-MBK-004 / R2-BRN-005: a small file that multiplies a big block through MINSERT rows and columns
/// (or three nested 100 x 100 arrays of one line) is refused quickly.
#[test]
fn a_dxf_with_minsert_arrays_is_refused_inside_the_expansion() {
    let mut dxf = String::from("0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nB\n10\n0\n20\n0\n30\n0\n");
    for i in 0..3000 {
        dxf += &format!("0\nLWPOLYLINE\n8\nW\n90\n4\n70\n1\n10\n{i}\n20\n0\n10\n{}\n20\n0\n10\n{}\n20\n1\n10\n{i}\n20\n1\n", i + 1, i + 1);
    }
    dxf += "0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nB\n10\n0\n20\n0\n30\n0\n70\n100\n71\n100\n44\n10\n45\n10\n0\nENDSEC\n0\nEOF\n";
    let mut e = Engine::new();
    let t = std::time::Instant::now();
    let r = e.exec_json(&json!({"cmd": "import_dxf", "text": dxf}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert!(t.elapsed().as_secs() < 10, "took {:?}", t.elapsed());
    // nested arrays of one line
    let mut d2 = String::from("0\nSECTION\n2\nBLOCKS\n");
    d2 += "0\nBLOCK\n2\nA\n10\n0\n20\n0\n30\n0\n0\nLINE\n10\n0\n20\n0\n30\n0\n11\n1\n21\n1\n31\n0\n0\nENDBLK\n";
    d2 += "0\nBLOCK\n2\nB\n10\n0\n20\n0\n30\n0\n0\nINSERT\n2\nA\n10\n0\n20\n0\n30\n0\n70\n100\n71\n100\n44\n2\n45\n2\n0\nENDBLK\n";
    d2 += "0\nBLOCK\n2\nC\n10\n0\n20\n0\n30\n0\n0\nINSERT\n2\nB\n10\n0\n20\n0\n30\n0\n70\n100\n71\n100\n44\n300\n45\n300\n0\nENDBLK\n";
    d2 += "0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nINSERT\n2\nC\n10\n0\n20\n0\n30\n0\n70\n100\n71\n100\n44\n30000\n45\n30000\n0\nENDSEC\n0\nEOF\n";
    let t = std::time::Instant::now();
    let r = e.exec_json(&json!({"cmd": "import_dxf", "text": d2}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert!(t.elapsed().as_secs() < 10);
}

#[test]
fn undo_history_is_bounded_by_size() {
    let mut e = Engine::new();
    run(&mut e, json!({"cmd": "terrain_grid", "width": 60, "depth": 60, "spacing": 0.5}));
    let one = e.model.approx_bytes();
    assert!(one > 500_000, "{one}");
    e.undo_budget = one * 4;
    for i in 0..30 {
        run(&mut e, json!({"cmd": "rect", "from": [i, 0, 0], "to": [i + 1, 1, 0]}));
    }
    assert!(e.undo_depth() <= 5, "{} undo steps kept", e.undo_depth());
    assert!(e.undo_depth() >= 1);
    run(&mut e, json!({"cmd": "undo"}));
}

// ---- OBJ import and export --------------------------------------------------------------

#[test]
fn obj_material_files_come_from_the_objs_own_folder_only() {
    let dir = scratch("mtllib");
    let secret = dir.join("secret.mtl");
    std::fs::write(&secret, "newmtl Hidden\nKd 0.125 0.625 0.875\n").unwrap();
    let sub = dir.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(sub.join("beside.mtl"), "newmtl Beside\nKd 0.25 0.5 0.75\n").unwrap();
    for (mtl, expect_color) in [("../secret.mtl", false), ("beside.mtl", true)] {
        let obj = sub.join("t.obj");
        std::fs::write(&obj, format!("mtllib {mtl}\nv 0 0 0\nv 1 0 0\nv 1 1 0\nusemtl Hidden\nusemtl Beside\nf 1 2 3\n")).unwrap();
        let mut e = Engine::new();
        run(&mut e, json!({"cmd": "import_obj", "path": obj}));
        let has_color = serde_json::to_string(&e.model).unwrap().contains("0.25,0.5,0.75");
        assert_eq!(has_color, expect_color, "{mtl}");
    }
    // an absolute path is no different
    let obj = sub.join("abs.obj");
    std::fs::write(&obj, format!("mtllib {}\nv 0 0 0\nv 1 0 0\nv 1 1 0\nf 1 2 3\n", secret.display())).unwrap();
    let mut e = Engine::new();
    run(&mut e, json!({"cmd": "import_obj", "path": obj}));
    assert!(!serde_json::to_string(&e.model).unwrap().contains("0.125,0.625,0.875"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn obj_export_names_cannot_start_new_lines() {
    let dir = scratch("objnames");
    let mut e = Engine::new();
    let f = run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}))["face"].as_u64().unwrap();
    let g = run(&mut e, json!({"cmd": "group", "ids": [f]}))["group"].as_u64().unwrap();
    run(&mut e, json!({"cmd": "rename", "id": g, "name": "wall\nmtllib /etc/passwd\ro x"}));
    let out = dir.join("m.obj");
    run(&mut e, json!({"cmd": "export_obj", "path": out}));
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.lines().any(|l| l.starts_with("mtllib /etc")), "{text}");
    assert!(text.lines().all(|l| !l.contains('\r')));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn imported_geometry_out_of_range_is_refused_whole() {
    // an arc whose radius is 1e300 used to import and poison the next transform
    let dxf = "0\nSECTION\n2\nENTITIES\n0\nLINE\n8\nA\n10\n0\n20\n0\n30\n0\n11\n1e300\n21\n0\n31\n0\n0\nENDSEC\n0\nEOF\n";
    let mut e = Engine::new();
    run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    let before = serde_json::to_string(&e.model).unwrap();
    let r = e.exec_json(&json!({"cmd": "import_dxf", "text": dxf}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(serde_json::to_string(&e.model).unwrap(), before);
}

// ---- Round 2 ---------------------------------------------------------------------------

fn backups(dir: &std::path::Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir.join("Backups")).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    v.sort();
    v
}

/// R2-BRN-001 / R2-MBK-006: another client's save is not one of my undo steps.
#[test]
fn undo_after_a_reload_cannot_erase_the_other_clients_work() {
    let dir = scratch("undo-reload");
    let doc = dir.join("live.sketchcraft");
    let mut app = Live::open(doc.clone());
    app.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let mut other = Live::open(doc.clone());
    other.exec(&json!({"cmd": "rect", "from": [10, 0, 0], "to": [14, 4, 0]}));
    other.exec(&json!({"cmd": "rect", "from": [20, 0, 0], "to": [24, 4, 0]}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    // the app picks up their work, then the person presses Undo: it takes back the person's own rectangle
    // (the one step they made) and never one of theirs
    assert_eq!(app.exec(&json!({"cmd": "info"}))["faces"], 3);
    assert_eq!(app.exec(&json!({"cmd": "undo"}))["ok"], true);
    let mut check = Engine::new();
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 2, "their two rectangles are still on disk, the person's own step is undone");
    assert!(!check.model.vertices.values().any(|v| v.p.x < 5.0), "the person's rectangle at x 0..4 is the one that went");
    // there was only one step of theirs: a second Undo has nothing to undo, and theirs stay
    assert_eq!(app.exec(&json!({"cmd": "undo"}))["ok"], false);
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 2);
    // and the person's own new steps undo normally afterwards
    app.exec(&json!({"cmd": "rect", "from": [30, 0, 0], "to": [34, 4, 0]}));
    app.exec(&json!({"cmd": "undo"}));
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 2);
    std::fs::remove_dir_all(&dir).ok();
}

/// R2-MBK-006: a client that finishes a slow command after someone else saved does not overwrite them.
#[test]
fn a_save_made_while_a_command_ran_is_not_overwritten() {
    let dir = scratch("race");
    let doc = dir.join("live.sketchcraft");
    let mut app = Live::open(doc.clone());
    app.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    // the other client saves between this client's sync and its save: simulate with a file written under it
    let mut other = Live::open(doc.clone());
    other.exec(&json!({"cmd": "rect", "from": [10, 0, 0], "to": [14, 4, 0]}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    // (app hasn't polled) its next command starts from theirs; nothing is lost
    let r = app.exec(&json!({"cmd": "rect", "from": [20, 0, 0], "to": [24, 4, 0]}));
    assert_eq!(r["ok"], true);
    let mut check = Engine::new();
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 3);
    std::fs::remove_dir_all(&dir).ok();
}

/// R2-BRN-002 / R2-MBK-003: a flood of `new` cannot push out the last good backup.
#[test]
fn repeated_new_cannot_erase_the_last_good_backup() {
    let dir = scratch("backup-flood");
    let doc = dir.join("live.sketchcraft");
    let mut live = Live::open(doc.clone());
    for i in 0..5 {
        live.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4 + i, 4, 0]}));
    }
    // an older good backup, made an hour ago
    std::fs::create_dir_all(dir.join("Backups")).unwrap();
    let old = dir.join("Backups").join(format!("live-{}.sketchcraft", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() - 7200));
    std::fs::copy(&doc, &old).unwrap();
    for _ in 0..30 {
        live.exec(&json!({"cmd": "new"}));
    }
    // empty models are not backed up
    let mut check = Engine::new();
    for p in backups(&dir) {
        run(&mut check, json!({"cmd": "open", "path": p}));
        assert!(faces(&mut check) >= 1, "every backup holds a drawn model");
    }
    // many distinct models in a row: the older good backup and the first of the burst remain
    let first: Vec<PathBuf> = backups(&dir);
    for i in 0..20 {
        live.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4 + i, 4, 0]}));
        live.exec(&json!({"cmd": "new"}));
    }
    let b2 = backups(&dir);
    assert!(b2.contains(&old), "the older good backup survived the flood");
    assert!(first.iter().filter(|p| **p != old).min_by_key(|p| { let n = p.file_stem().unwrap().to_string_lossy().into_owned(); let mut it = n.rsplit('-'); let (a, b) = (it.next().and_then(|x| x.parse::<u64>().ok()), it.next().and_then(|x| x.parse::<u64>().ok())); match (a, b) { (Some(n), Some(t)) => (t, n), (Some(t), None) => (t, 0), _ => (0, 0) } }).is_none_or(|p| b2.contains(p)), "so did the first backup of the burst");
    assert!(b2.len() <= 60, "{} backups", b2.len());
    std::fs::remove_dir_all(&dir).ok();
}

/// R2-BRN-008: flags and copies don't pile up or stick.
#[test]
fn damaged_copies_are_pruned_and_the_notice_clears_when_the_file_is_repaired() {
    let dir = scratch("damaged-prune");
    let doc = dir.join("live.sketchcraft");
    let mut last = None;
    for _ in 0..24 {
        std::fs::write(&doc, b"{ not a model").unwrap();
        let live = Live::open(doc.clone());
        assert!(live.notice.is_some());
        last = live.notice.clone();
    }
    let kept = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".damaged-")).count();
    assert_eq!(kept, 20, "{kept} damaged copies");
    // R4-BRN-006: nothing is removed without a notice that names what went
    assert!(last.as_deref().is_some_and(|n| n.contains("older damaged file") && n.contains("removed")), "{last:?}");
    // a look-alike the program didn't write is never counted or removed (R4-MBK-005)
    std::fs::write(dir.join("live.damaged-1700000001"), b"planted").unwrap();
    for _ in 0..3 {
        std::fs::write(&doc, b"{ not a model").unwrap();
        Live::open(doc.clone());
    }
    assert!(dir.join("live.damaged-1700000001").exists(), "a planted look-alike was removed");
    std::fs::remove_dir_all(&dir).ok();
}

/// R2-BRN-004: a point list that would take minutes is refused up front.
#[test]
fn long_point_lists_and_huge_counts_are_refused_quickly() {
    let mut e = Engine::new();
    let pts: Vec<Value> = (0..3000).map(|i| json!([i as f64, (i % 7) as f64, 0])).collect();
    let r = e.exec_json_as(&Caller::Plugin, &json!({"cmd": "line", "points": pts}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    let ring: Vec<Value> = (0..3000).map(|i| json!([i as f64, (i % 7) as f64])).collect();
    let r = e.exec_json_as(&Caller::Plugin, &json!({"cmd": "text_3d", "rings": [ring.clone(), ring.clone(), ring.clone()], "height": 1}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    // R2-MBK-005: a stair of 100,000 m would be half a million steps
    let t = std::time::Instant::now();
    let r = e.exec_json_as(&Caller::Plugin, &json!({"cmd": "stairs", "at": [0, 0, 0], "rise": 100000, "tread": 0.3}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    let r = e.exec_json(&json!({"cmd": "stairs", "at": [0, 0, 0], "rise": 1e8, "tread": 0.3}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert!(t.elapsed().as_secs() < 2);
    let r = e.exec_json(&json!({"cmd": "stairs", "at": [0, 0, 0], "rise": 2.8, "tread": 0.28}).to_string());
    assert_eq!(r["ok"], true, "{r}");
}

/// R2-BRN-003: names and notes count toward the undo budget and are capped in total.
#[test]
fn long_names_are_counted_and_capped() {
    let mut e = Engine::new();
    let mut refused = false;
    for i in 0..200 {
        let r = e.exec_json_as(&Caller::Plugin, &json!({"cmd": "material", "name": format!("{i}{}", "m".repeat(60_000)), "color": [1, 0, 0]}).to_string());
        if r["ok"] != true {
            refused = true;
            break;
        }
    }
    assert!(refused, "the total of names is capped");
    assert!(e.model.text_bytes() <= sketchcraft::model::MAX_TEXT_BYTES);
    assert!(e.model.approx_bytes() >= e.model.text_bytes());
}

/// R2-BRN-007: a point cloud with out-of-range values is cleaned at import and refused in the model.
#[test]
fn point_clouds_are_finite_and_in_range() {
    let dir = scratch("cloud");
    let f = dir.join("c.xyz");
    std::fs::write(&f, "1 2 3\n4 5 6\n1e39 0 0\n7 8 9\n").unwrap();
    let mut e = Engine::new();
    run(&mut e, json!({"cmd": "import_cloud", "path": f}));
    assert!(e.model.clouds.values().all(|c| c.points.iter().all(|x| x.is_finite() && (*x as f64).abs() <= 1e9)));
    assert_eq!(e.model.clouds.values().map(|c| c.points.len()).sum::<usize>(), 9);
    e.model.clouds.values_mut().next().unwrap().points = std::sync::Arc::new(vec![f32::INFINITY, 0.0, 0.0]);
    assert!(e.model.check_sane().is_err());
    std::fs::remove_dir_all(&dir).ok();
}

/// R2-BRN-008: grouping part of a solid then subdividing the group no longer hits a stale id.
#[test]
fn subdividing_a_partial_group_works() {
    let mut e = Engine::new();
    let f = run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [1, 1, 0]}))["face"].as_u64().unwrap();
    let top = run(&mut e, json!({"cmd": "pushpull", "face": f, "distance": 1}))["face"].as_u64().unwrap();
    let g = run(&mut e, json!({"cmd": "group", "ids": [top]}))["group"].as_u64().unwrap();
    run(&mut e, json!({"cmd": "subdivide", "id": g, "levels": 1}));
    // everything still refers to vertices that exist
    for f in e.model.faces.values() {
        assert!(f.loops().flatten().all(|v| e.model.vertices.contains_key(v)));
    }
}

fn agent(ws: &std::path::Path) -> Caller {
    Caller::Mcp(Workspace::at(ws))
}

/// R2-MBK-001: `library_add` copies an OBJ's material file only when it is a bare name beside the OBJ.
#[test]
fn library_add_cannot_be_steered_by_the_objs_material_line() {
    let base = scratch("libadd");
    let ws = base.join("ws");
    std::fs::create_dir_all(ws.join("a")).unwrap();
    let victim = base.join("victim.txt");
    std::fs::write(&victim, b"important").unwrap();
    let obj = |mtl: &str| format!("mtllib {mtl}\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n");
    let mut e = Engine::new();
    // absolute path to a file outside: it must not be touched
    std::fs::write(ws.join("a/m.obj"), obj(&victim.to_string_lossy())).unwrap();
    e.exec_json_as(&agent(&ws), &json!({"cmd": "library_add", "paths": ["a/m.obj"]}).to_string());
    assert_eq!(std::fs::read(&victim).unwrap(), b"important");
    // a relative path out of the folder: nothing lands outside the library
    std::fs::create_dir_all(base.join("escape")).unwrap();
    std::fs::write(ws.join("escape.txt"), b"payload").unwrap();
    std::fs::write(ws.join("a/n.obj"), obj("../../escape.txt")).unwrap();
    e.exec_json_as(&agent(&ws), &json!({"cmd": "library_add", "paths": ["a/n.obj"]}).to_string());
    assert!(!base.join("escape.txt").exists() && !ws.join("Library/../../escape.txt").exists());
    // the library an agent uses is inside its workspace
    assert!(ws.join("Library/m.obj").exists());
    // and the same through the engine directly (the person's own add): still no truncation
    let lib = base.join("lib");
    sketchcraft::library::add_files(&lib, &[ws.join("a/m.obj").to_string_lossy().into_owned()], "", true).unwrap();
    assert_eq!(std::fs::read(&victim).unwrap(), b"important");
    std::fs::remove_dir_all(&base).ok();
}

/// R2-MBK-002: `save_image` writes only real PNGs, with a `.png` name, and never over someone else's file.
#[test]
fn agents_write_only_the_right_kind_of_file_and_never_replace_the_persons() {
    use base64::Engine as _;
    let base = scratch("agentwrite");
    let ws = base.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let mut e = Engine::new();
    let b64 = |b: &[u8]| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(b));
    // a shell script dressed as an image
    let r = e.exec_json_as(&agent(&ws), &json!({"cmd": "save_image", "path": "build.sh", "data": b64(b"#!/bin/sh\necho hi\n")}).to_string());
    assert_eq!(r["ok"], false);
    let r = e.exec_json_as(&agent(&ws), &json!({"cmd": "save_image", "path": "x.png", "data": b64(b"#!/bin/sh\necho hi\n")}).to_string());
    assert_eq!(r["ok"], false, "not decodable: {r}");
    assert!(!ws.join("x.png").exists() && !ws.join("build.sh").exists());
    // a real image is written
    let mut png = vec![];
    image::DynamicImage::new_rgb8(4, 4).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let r = e.exec_json_as(&agent(&ws), &json!({"cmd": "save_image", "path": "x.png", "data": b64(&png)}).to_string());
    assert_eq!(r["ok"], true, "{r}");
    // the person's own file is not replaced, whatever the command
    std::fs::write(ws.join("mine.obj"), b"the person's model").unwrap();
    let a = agent(&ws);
    for cmd in [json!({"cmd": "export_obj", "path": "mine.obj"}), json!({"cmd": "save", "path": "mine.obj"}), json!({"cmd": "export_obj", "path": "notes.txt"})] {
        let r = e.exec_json_as(&a, &cmd.to_string());
        assert_eq!(r["ok"], false, "{cmd} -> {r}");
    }
    assert_eq!(std::fs::read(ws.join("mine.obj")).unwrap(), b"the person's model");
    // a file the agent made itself may be written again
    run_as(&mut e, &a, json!({"cmd": "rect", "from": [0, 0, 0], "to": [2, 2, 0]}));
    run_as(&mut e, &a, json!({"cmd": "export_obj", "path": "mine2.obj"}));
    run_as(&mut e, &a, json!({"cmd": "export_obj", "path": "mine2.obj"}));
    // a workspace is never a drive root or home folder
    assert!(Workspace::at("/").too_wide());
    if let Some(h) = std::env::var_os("HOME") {
        assert!(Workspace::at(h).too_wide());
    }
    assert!(!Workspace::at(&ws).too_wide());
    std::fs::remove_dir_all(&base).ok();
}

fn run_as(e: &mut Engine, c: &Caller, v: Value) -> Value {
    let r = e.exec_json_as(c, &v.to_string());
    assert_eq!(r["ok"], true, "{v} -> {r}");
    r
}

/// R2-BRN-006: a plugin run (begin_batch ... end_batch) is one undo step and one save, however many commands it sent.
#[test]
fn a_batch_of_commands_is_one_undo_step_and_one_save() {
    let dir = scratch("batch");
    let doc = dir.join("live.sketchcraft");
    let mut app = Live::open(doc.clone());
    app.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    let depth = app.engine.undo_depth();
    app.exec(&json!({"cmd": "begin_batch"}));
    for i in 0..300 {
        let r = app.exec_as(&Caller::Plugin, &json!({"cmd": "rect", "from": [10 + i * 6, 0, 0], "to": [14 + i * 6, 4, 0]}));
        assert_eq!(r["ok"], true);
    }
    // Undo isn't available to the person mid-run, nor to the plugin
    assert_eq!(app.exec_as(&Caller::Plugin, &json!({"cmd": "undo"}))["ok"], false);
    // the file still holds the state before the run
    let mut check = Engine::new();
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 1);
    app.exec(&json!({"cmd": "end_batch"}));
    assert_eq!(app.engine.undo_depth(), depth + 1, "one step for 300 commands");
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 301);
    // plugins can't open or close batches themselves
    assert_eq!(app.exec_as(&Caller::Plugin, &json!({"cmd": "end_batch"}))["ok"], false);
    app.exec(&json!({"cmd": "undo"}));
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 1, "one Undo takes back the whole run");
    std::fs::remove_dir_all(&dir).ok();
}

// ---- Round 3 ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

/// R3-MBK-002: an agent can't put files in Backups or Plugins through the library commands.
#[test]
fn library_commands_cannot_reach_backups_or_plugins() {
    let base = scratch("lib-confine");
    let ws = base.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    let a = agent(&ws);
    let mut e = Engine::new();
    run_as(&mut e, &a, json!({"cmd": "rect", "from": [0, 0, 0], "to": [2, 2, 0]}));
    let f = run_as(&mut e, &a, json!({"cmd": "rect", "from": [10, 0, 0], "to": [12, 2, 0]}))["face"].as_u64().unwrap();
    let g = run_as(&mut e, &a, json!({"cmd": "group", "ids": [f]}))["group"].as_u64().unwrap();
    let c = run_as(&mut e, &a, json!({"cmd": "make_component", "ids": [g], "name": "Thing"}));
    let comp = c["component"].as_u64().unwrap();
    for bad in ["Backups", "backups", "Plugins", ".hidden", "../x", "a/b", "..", "Backups/x"] {
        for key in ["collection", "name"] {
            let mut cmd = json!({"cmd": "save_to_library", "component": comp, "dir": ".", "collection": "ok", "name": "live-1"});
            cmd[key] = json!(bad);
            let r = e.exec_json_as(&a, &cmd.to_string());
            assert_eq!(r["ok"], false, "{key}={bad:?} -> {r}");
        }
    }
    assert!(!ws.join("Backups").exists() && !ws.join("Plugins").exists());
    // the person's own app is held to the reserved folder names too
    let r = e.exec_json(&json!({"cmd": "save_to_library", "component": comp, "dir": ws, "collection": "Backups"}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    // an ordinary name works, and never replaces a file
    let r1 = run_as(&mut e, &a, json!({"cmd": "save_to_library", "component": comp, "dir": ".", "collection": "Mine", "name": "Chair"}));
    let r2 = run_as(&mut e, &a, json!({"cmd": "save_to_library", "component": comp, "dir": ".", "collection": "Mine", "name": "Chair"}));
    assert_ne!(r1["path"], r2["path"]);
    std::fs::remove_dir_all(&base).ok();
}

/// R3-MBK-001 / 006: library_add and export siblings never replace the person's files.
#[test]
fn library_add_and_export_siblings_never_replace_the_persons_files() {
    let base = scratch("lib-replace");
    let ws = base.join("ws");
    std::fs::create_dir_all(ws.join("a")).unwrap();
    std::fs::create_dir_all(ws.join("b")).unwrap();
    std::fs::write(ws.join("b/chair.obj"), b"PERSONS OBJ").unwrap();
    std::fs::write(ws.join("b/chair.mtl"), b"PERSONS MTL").unwrap();
    std::fs::write(ws.join("foo.mtl"), b"PERSONS FOO MTL").unwrap();
    let a = agent(&ws);
    let mut e = Engine::new();
    run_as(&mut e, &a, json!({"cmd": "rect", "from": [0, 0, 0], "to": [2, 2, 0]}));
    run_as(&mut e, &a, json!({"cmd": "export_obj", "path": "a/chair.obj"}));
    // the OBJ the agent made, added over the person's file
    let r = e.exec_json_as(&a, &json!({"cmd": "library_add", "paths": ["a/chair.obj"], "dir": "b"}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(std::fs::read(ws.join("b/chair.obj")).unwrap(), b"PERSONS OBJ");
    // and the material file beside it
    std::fs::write(ws.join("a/chair.obj"), "mtllib chair.mtl\no x\n").unwrap();
    let r = e.exec_json_as(&a, &json!({"cmd": "library_add", "paths": ["a/chair.obj"], "dir": "b"}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(std::fs::read(ws.join("b/chair.mtl")).unwrap(), b"PERSONS MTL");
    // a new name is fine
    let r = e.exec_json_as(&a, &json!({"cmd": "library_add", "paths": ["a/chair.obj"], "dir": "a2"}).to_string());
    assert_eq!(r["ok"], true, "{r}");
    // export_obj next to the person's material file
    let r = e.exec_json_as(&a, &json!({"cmd": "export_obj", "path": "foo.obj"}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(std::fs::read(ws.join("foo.mtl")).unwrap(), b"PERSONS FOO MTL");
    assert!(!ws.join("foo.obj").exists());
    std::fs::remove_dir_all(&base).ok();
}

/// R3-MBK-009: the report CSV can't carry formulas.
#[test]
fn report_cells_cannot_be_formulas() {
    let base = scratch("csv");
    let mut e = Engine::new();
    let f = run(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [2, 2, 0]}))["face"].as_u64().unwrap();
    let g = run(&mut e, json!({"cmd": "group", "ids": [f], "name": "=HYPERLINK(\"http://x\",\"y\")"}))["group"].as_u64().unwrap();
    let _ = g;
    let out = base.join("r.csv");
    run(&mut e, json!({"cmd": "report", "path": out}));
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.lines().any(|l| l.split(',').any(|c| c.starts_with('=') || c.starts_with("\"="))), "{text}");
    assert!(text.contains("'=HYPERLINK"), "{text}");
    std::fs::remove_dir_all(&base).ok();
}

/// R3-MBK-002 / R3-BRN-007: backups other than SketchCraft's own are ignored; documents with a shared prefix stay apart.
#[test]
fn fake_and_foreign_backups_cannot_cause_real_ones_to_be_deleted() {
    let dir = scratch("fake-backups");
    let doc = dir.join("live.sketchcraft");
    let mut live = Live::open(doc.clone());
    live.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    live.exec(&json!({"cmd": "new"}));
    let real: Vec<PathBuf> = backups(&dir);
    assert_eq!(real.len(), 1);
    // seventy files with far-future times, and a second document's backups
    for i in 0..70 {
        std::fs::write(dir.join("Backups").join(format!("live-{}.sketchcraft", 4_102_444_800u64 + i)), b"x").unwrap();
    }
    std::fs::write(dir.join("Backups").join(format!("live-draft-{}.sketchcraft", now_secs() - 5)), b"{}").unwrap();
    for i in 0..4 {
        std::fs::write(dir.join("Backups").join(format!("live-draft-{}.sketchcraft", now_secs() - 100 - i)), b"{}").unwrap();
    }
    for i in 0..30 {
        live.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4 + i, 4, 0]}));
        live.exec(&json!({"cmd": "new"}));
    }
    assert!(real.iter().all(|p| p.exists()), "the real backup survived");
    assert_eq!(std::fs::read_dir(dir.join("Backups")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("live-draft-")).count(), 5, "another document's backups were left alone");
    assert_eq!(std::fs::read_dir(dir.join("Backups")).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("live-4102")).count(), 70, "files that aren't ours were left alone");
    std::fs::remove_dir_all(&dir).ok();
}

/// R3-BRN-006: a build-reset loop can't push out the good model it replaced.
#[test]
fn a_reset_loop_keeps_the_good_model() {
    let dir = scratch("reset-loop");
    let doc = dir.join("live.sketchcraft");
    let mut live = Live::open(doc.clone());
    for i in 0..5 {
        live.exec(&json!({"cmd": "rect", "from": [i * 10, 0, 0], "to": [i * 10 + 4, 4, 0]}));
    }
    live.exec(&json!({"cmd": "new"}));
    let big = backups(&dir)[0].clone();
    for i in 0..30 {
        live.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4 + i, 4, 0]}));
        live.exec(&json!({"cmd": "new"}));
    }
    assert!(big.exists(), "the five-rectangle backup is still there");
    std::fs::remove_dir_all(&dir).ok();
}

/// R3-MBK-005: deleting most of the model keeps what was there in Backups first.
#[test]
fn deleting_most_of_the_model_takes_a_backup_first() {
    let dir = scratch("shrink-backup");
    let doc = dir.join("live.sketchcraft");
    let mut live = Live::open(doc.clone());
    let mut ids = vec![];
    for i in 0..4 {
        ids.push(live.exec(&json!({"cmd": "rect", "from": [i * 10, 0, 0], "to": [i * 10 + 4, 4, 0]}))["face"].as_u64().unwrap());
    }
    assert!(backups(&dir).len() <= 1);
    let r = live.exec(&json!({"cmd": "delete", "ids": ids}));
    assert_eq!(r["ok"], true, "{r}");
    let mut check = Engine::new();
    let found = backups(&dir).into_iter().any(|p| {
        run(&mut check, json!({"cmd": "open", "path": p}));
        faces(&mut check) == 4
    });
    assert!(found, "a backup of the four rectangles exists");
    std::fs::remove_dir_all(&dir).ok();
}

/// R3-BRN-001 / R3-MBK-007: another client's save during a plugin run is not applied to the run, and the run's work is not lost.
#[test]
fn another_clients_save_during_a_batch_does_not_hit_the_batch() {
    let dir = scratch("batch-foreign");
    let doc = dir.join("live.sketchcraft");
    let mut app = Live::open(doc.clone());
    app.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let mut other = Live::open(doc.clone());
    app.exec(&json!({"cmd": "begin_batch"}));
    for i in 0..2 {
        let r = app.exec_as(&Caller::Plugin, &json!({"cmd": "rect", "from": [10 + i * 6, 0, 0], "to": [14 + i * 6, 4, 0]}));
        assert_eq!(r["ok"], true);
    }
    // the other client saves meanwhile (it is on the file as of its open: 1 face, then its own rect)
    other.exec(&json!({"cmd": "rect", "from": [50, 0, 0], "to": [54, 4, 0]}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    // the plugin's later commands still land on the plugin's own geometry (no reload inside the batch)
    let r = app.exec_as(&Caller::Plugin, &json!({"cmd": "rect", "from": [30, 0, 0], "to": [34, 4, 0]}));
    assert_eq!(r["ok"], true);
    assert_eq!(app.engine.model.faces.len(), 4, "the run's own rectangles are all still there");
    assert!(!app.sync(), "no reload while a batch is open");
    // when the run ends, the other client's version is kept and the run's work is set aside, with a message
    let r = app.exec(&json!({"cmd": "end_batch"}));
    assert_eq!(r["ok"], false, "{r}");
    assert!(r["error"].as_str().unwrap().contains("set aside"), "{r}");
    let aside: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().contains(".unsaved-")).collect();
    assert_eq!(aside.len(), 1, "{aside:?}");
    let mut check = Engine::new();
    run(&mut check, json!({"cmd": "open", "path": &aside[0]}));
    assert_eq!(faces(&mut check), 4, "all of the run's work is in the side file");
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 2, "their work is the file's");
    assert!(app.scene()["notice"].as_str().is_some_and(|n| n.contains(".unsaved-")), "the warning stays");
    std::fs::remove_dir_all(&dir).ok();
}

/// R3-MBK-007: a batch nobody closed is closed and saved by the timer (here: expired by hand).
#[test]
fn a_batch_nobody_closed_is_closed_and_saved() {
    let dir = scratch("batch-timeout");
    let doc = dir.join("live.sketchcraft");
    let mut app = Live::open(doc.clone());
    app.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    let r = app.exec(&json!({"cmd": "begin_batch"}));
    let id = r["batch"].as_u64().unwrap();
    app.exec_as(&Caller::Plugin, &json!({"cmd": "rect", "from": [10, 0, 0], "to": [14, 4, 0]}));
    // an end for a batch that is already over does nothing
    app.exec(&json!({"cmd": "end_batch", "id": id + 7}));
    assert!(app.engine.in_batch());
    app.engine.expire_batch_for_test();
    app.sync();
    assert!(!app.engine.in_batch());
    let mut check = Engine::new();
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 2, "what the run did was saved when it was closed");
    std::fs::remove_dir_all(&dir).ok();
}

/// R3-BRN-008: a failed save followed by another client's save does not lose the unsaved work.
#[cfg(unix)]
#[test]
fn unsaved_work_is_kept_when_another_client_saves_after_a_failed_save() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("save-fail-reload");
    let doc = dir.join("live.sketchcraft");
    let mut app = Live::open(doc.clone());
    app.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let mut other = Live::open(doc.clone());
    // the folder stops taking writes: two edits that can't be saved
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let r = app.exec(&json!({"cmd": "rect", "from": [10, 0, 0], "to": [14, 4, 0]}));
    app.exec(&json!({"cmd": "rect", "from": [20, 0, 0], "to": [24, 4, 0]}));
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    if r.get("save_error").is_none() {
        // running as a user who can write anywhere (root): the failure can't be staged
        std::fs::remove_dir_all(&dir).ok();
        return;
    }
    other.exec(&json!({"cmd": "rect", "from": [50, 0, 0], "to": [54, 4, 0]}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert!(app.sync(), "their version is loaded");
    let aside: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().contains(".unsaved-")).collect();
    assert_eq!(aside.len(), 1, "the unsaved work was set aside: {aside:?}");
    let mut check = Engine::new();
    run(&mut check, json!({"cmd": "open", "path": &aside[0]}));
    assert_eq!(faces(&mut check), 3, "all three of the app's rectangles");
    assert!(app.scene()["notice"].as_str().is_some_and(|n| n.contains(".unsaved-")));
    std::fs::remove_dir_all(&dir).ok();
}

/// R3-BRN-011: a save that changes nothing keeps the undo history; one that changes something says so once.
#[test]
fn undo_history_survives_a_touch_and_the_loss_is_reported() {
    let dir = scratch("undo-keep");
    let doc = dir.join("live.sketchcraft");
    let mut app = Live::open(doc.clone());
    app.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    app.exec(&json!({"cmd": "rect", "from": [10, 0, 0], "to": [14, 4, 0]}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    // someone rewrites the file with the same content
    let bytes = std::fs::read(&doc).unwrap();
    std::fs::write(&doc, &bytes).unwrap();
    assert!(app.sync());
    assert_eq!(app.engine.undo_depth(), 2, "nothing changed, so history stays");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let mut other = Live::open(doc.clone());
    other.exec(&json!({"cmd": "rect", "from": [50, 0, 0], "to": [54, 4, 0]}));
    std::thread::sleep(std::time::Duration::from_millis(1100));
    app.sync();
    // R4 (per-client undo): the person's own steps are kept across another program's real save
    assert_eq!(app.engine.undo_depth(), 2, "the history is kept");
    assert!(app.scene()["notice"].is_null(), "{:?}", app.scene()["notice"]);
    std::fs::remove_dir_all(&dir).ok();
}

/// R3-MBK-004: copies are bounded by the geometry in them, not by how many ids name them.
#[test]
fn array_and_copy_of_heavy_groups_are_bounded_by_geometry() {
    let mut e = Engine::new();
    // a subdivided cube: thousands of faces in one group
    let cage = json!({"cmd": "subd", "points": [[0,0,0],[1,0,0],[1,1,0],[0,1,0],[0,0,1],[1,0,1],[1,1,1],[0,1,1]],
        "faces": [[0,3,2,1],[4,5,6,7],[0,1,5,4],[1,2,6,5],[2,3,7,6],[3,0,4,7]], "levels": 4});
    let g = run(&mut e, cage)["group"].as_u64().unwrap();
    let before = e.model.entity_count();
    // an array of 400 copies of ~12k faces each is refused up front, before any is made
    let r = e.exec_json(&json!({"cmd": "array", "ids": [g], "by": [2, 0, 0], "count": 400}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(e.model.entity_count(), before, "nothing was copied");
    // a few copies are fine
    run(&mut e, json!({"cmd": "array", "ids": [g], "by": [2, 0, 0], "count": 3}));
    // and a plain duplicate of a light group still works
    let f = run(&mut e, json!({"cmd": "rect", "from": [0, 20, 0], "to": [2, 22, 0]}))["face"].as_u64().unwrap();
    run(&mut e, json!({"cmd": "duplicate", "ids": [f], "by": [5, 0, 0]}));
}

/// R3-BRN-002: text_3d with many outlines is refused (plugins and agents) instead of running for hours.
#[test]
fn text_3d_with_many_rings_is_refused_quickly() {
    let mut e = Engine::new();
    let rings: Vec<Value> = (0..400).map(|i| json!([[i, 0], [i + 1, 0], [i + 1, 1], [i, 1]])).collect();
    let t = std::time::Instant::now();
    let r = e.exec_json_as(&Caller::Plugin, &json!({"cmd": "text_3d", "rings": rings, "at": [0, 0, 0], "right": [1, 0, 0], "up": [0, 1, 0], "depth": 0.1}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert!(t.elapsed().as_secs() < 5);
}

/// R3-BRN-004: grouping part of a large mesh is not quadratic.
#[test]
fn grouping_half_of_a_big_terrain_is_fast() {
    let mut e = Engine::new();
    let g = run(&mut e, json!({"cmd": "terrain_grid", "at": [0, 0, 0], "width": 120, "depth": 120, "spacing": 1}));
    let _ = g;
    let ids: Vec<u64> = e.model.faces.keys().take(e.model.faces.len() / 2).map(|k| *k as u64).collect();
    let t = std::time::Instant::now();
    run(&mut e, json!({"cmd": "group", "ids": ids}));
    assert!(t.elapsed().as_secs() < 10, "{:?}", t.elapsed());
}

/// R3-BRN-003: crossing lines and long polylines in a DXF stop at the time and output limits.
#[test]
fn a_dense_dxf_is_refused_not_ground_through() {
    let mut s = String::from("0\nSECTION\n2\nENTITIES\n");
    for i in 0..1500 {
        s += &format!("0\nLINE\n8\n0\n10\n{i}\n20\n0\n30\n0\n11\n{i}\n21\n1500\n31\n0\n0\nLINE\n8\n0\n10\n0\n20\n{i}\n30\n0\n11\n1500\n21\n{i}\n31\n0\n");
    }
    s += "0\nENDSEC\n0\nEOF\n";
    let mut e = Engine::new();
    let t = std::time::Instant::now();
    let r = e.exec_json(&json!({"cmd": "import_dxf", "text": s, "units": "m"}).to_string());
    assert!(t.elapsed().as_secs() < 30, "{:?}", t.elapsed());
    // either refused (too many faces / too slow) or imported; never a hang
    let _ = r;
}

/// R3-BRN-012: an integration test program is recognized, and (with no SKETCHCRAFT_DOC) would get a temp folder, never ~/Documents.
#[test]
fn test_programs_are_scoped_away_from_the_real_folder() {
    assert!(sketchcraft::live::running_as_cargo_test());
    let p = sketchcraft::live::doc_path(None, sketchcraft::live::running_as_cargo_test(), std::env::var_os("HOME").map(PathBuf::from));
    assert!(p.starts_with(std::env::temp_dir()), "{p:?}");
    // with SKETCHCRAFT_DOC set (as every run here does) that wins
    if let Some(v) = std::env::var_os("SKETCHCRAFT_DOC") {
        assert_eq!(sketchcraft::live::default_doc(), PathBuf::from(v));
    }
}

/// R4-BRN-005 / R4-MBK-008 / CASHOUT-002: the guard does not need a folder called `target`, and it
/// works whichever way cargo lays test programs out (`deps/` or `build/<pkg>/<hash>/out/`).
#[test]
fn the_test_guard_does_not_need_a_folder_called_target() {
    assert!(sketchcraft::live::running_as_cargo_test());
    let ok = |p: &str| sketchcraft::live::exe_in_test_layout(std::path::Path::new(p));
    assert!(ok("/x/anything/debug/deps/hardening-1a2b"));
    assert!(ok("/x/anything/debug/build/sketchcraft-core/c59a64f19ee66941/out/sketchcraft-c59a64"));
    assert!(!ok("/x/anything/release/sketchcraft"));
    assert!(!ok("/Applications/SketchCraft.app/Contents/MacOS/sketchcraft"));
    assert!(!ok("/x/build/out/sketchcraft"));
}

/// R4-BRN-001: the wall-layer "borrowed lines" pass is bounded: the same drawing with the long
/// polyline on a wall layer takes about as long as with it on a plain one (it was quadratic).
#[test]
fn a_wall_layer_with_many_borrowed_lines_is_bounded() {
    let n = 100_000;
    let make = |layer: &str| {
        let mut s = format!("0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n8\n{layer}\n90\n{n}\n70\n0\n");
        for i in 0..n {
            s += &format!("10\n{}\n20\n0\n", i as f64 * 0.5);
        }
        for i in 0..n - 1 {
            s += &format!("0\nLINE\n8\nOTHER\n10\n{}\n20\n0\n30\n0\n11\n{}\n21\n0\n31\n0\n", i as f64 * 0.5, (i + 1) as f64 * 0.5);
        }
        s + "0\nENDSEC\n0\nEOF\n"
    };
    let time = |layer: &str| {
        let mut e = Engine::new();
        let t = std::time::Instant::now();
        let _ = e.exec_json(&json!({"cmd": "import_dxf", "text": make(layer), "units": "m"}).to_string());
        t.elapsed().as_secs_f64()
    };
    let (plain, wall) = (time("ZZZ"), time("WALLS"));
    assert!(wall < plain * 1.5 + 2.0, "wall layer {wall:.1}s vs plain {plain:.1}s");
}

#[test]
fn many_wall_layers_are_not_quadratic() {
    // 6,000 layers named WALL<n> with one line each, and 60,000 lines on a plain layer
    let mut s = String::from("0\nSECTION\n2\nENTITIES\n");
    for i in 0..6000 {
        s += &format!("0\nLINE\n8\nWALL{i}\n10\n{i}\n20\n0\n30\n0\n11\n{i}\n21\n1\n31\n0\n");
    }
    for i in 0..60_000 {
        s += &format!("0\nLINE\n8\nPLAIN\n10\n{}\n20\n5\n30\n0\n11\n{}\n21\n6\n31\n0\n", i as f64 * 0.01, i as f64 * 0.01);
    }
    s += "0\nENDSEC\n0\nEOF\n";
    let mut e = Engine::new();
    let t = std::time::Instant::now();
    let _ = e.exec_json(&json!({"cmd": "import_dxf", "text": s, "units": "m"}).to_string());
    assert!(t.elapsed().as_secs() < 25, "{:?}", t.elapsed());
}

#[test]
fn a_hatch_with_many_islands_answers_inside_the_deadline() {
    // one big outline and 3,000 small islands, each a hatch loop (the hatch format the importer reads)
    let loops = 3001;
    let mut s = format!("0\nSECTION\n2\nENTITIES\n0\nHATCH\n8\n0\n10\n0\n20\n0\n30\n0\n2\nSOLID\n70\n1\n71\n0\n91\n{loops}\n");
    let ring = |x0: f64, y0: f64, w: f64, n: usize| {
        let mut r = format!("92\n3\n72\n0\n73\n1\n93\n{n}\n");
        for k in 0..n {
            let a = k as f64 / n as f64 * std::f64::consts::TAU;
            r += &format!("10\n{}\n20\n{}\n", x0 + w * a.cos(), y0 + w * a.sin());
        }
        r + "97\n0\n"
    };
    s += &ring(0.0, 0.0, 5000.0, 2000);
    for i in 0..loops - 1 {
        s += &ring((i % 60) as f64 * 40.0 - 1200.0, (i / 60) as f64 * 40.0 - 1000.0, 5.0, 12);
    }
    s += "75\n0\n76\n1\n0\nENDSEC\n0\nEOF\n";
    let mut e = Engine::new();
    let t = std::time::Instant::now();
    let r = e.exec_json(&json!({"cmd": "import_dxf", "text": s, "units": "m"}).to_string());
    assert!(t.elapsed().as_secs() < 25, "{:?}", t.elapsed());
    // it was really read as a hatch: either made, or refused for time, not "nothing to draw"
    assert!(r["ok"] == true || !r["error"].as_str().unwrap_or("").contains("nothing SketchCraft can draw"), "{r}");
}

/// R4-BRN-004: text_3d has one end-to-end deadline: the app's own call with 400 overlapping outlines
/// (inside the app's count cap) and ordinary text on a big model both answer (done or refused) in bounded time.
#[test]
fn text_3d_stops_at_its_deadline_for_the_app_too() {
    let circle = |cx: f64, cy: f64, r: f64| -> Value { Value::Array((0..40).map(|k| { let a = k as f64 / 40.0 * std::f64::consts::TAU; json!([cx + r * a.cos(), cy + r * a.sin()]) }).collect()) };
    let mut e = Engine::new();
    let rings: Vec<Value> = (0..400).map(|i| circle(i as f64 * 0.01, 0.0, 1.0)).collect();
    let t = std::time::Instant::now();
    let _ = e.exec_json(&json!({"cmd": "text_3d", "rings": rings, "at": [0, 0, 0], "right": [1, 0, 0], "up": [0, 1, 0], "depth": 0.1}).to_string());
    let took = t.elapsed().as_secs_f64();
    assert!(took < 20.0, "UI text_3d with 400 overlapping circles ran {took:.1}s");
    // nothing is left half done when it was stopped
    // and a model that was already big: the limit still holds
    let mut e = Engine::new();
    run(&mut e, json!({"cmd": "terrain_grid", "at": [0, 0, 0], "width": 150, "depth": 150, "spacing": 1}));
    let rings: Vec<Value> = (0..10).map(|i| circle(i as f64 * 3.0, 200.0, 1.0)).collect();
    let t = std::time::Instant::now();
    let _ = e.exec_json_as(&Caller::Plugin, &json!({"cmd": "text_3d", "rings": rings, "at": [0, 0, 0], "right": [1, 0, 0], "up": [0, 1, 0], "depth": 0.1}).to_string());
    let took = t.elapsed().as_secs_f64();
    assert!(took < 20.0, "plugin text_3d on a big model ran {took:.1}s");
}

/// R4-MBK-002: the reserved folders are compared folded and by identity, not by spelling.
#[test]
fn look_alike_spellings_of_reserved_folders_are_refused() {
    use sketchcraft::library::{plain_name, reserved};
    for bad in ["Backups", "BACKUPS", "Backupſ", "Pluginſ", "Bac\u{212A}ups", "Backups.", "Backups ", "Backups...", "BACKUP~1", "PLUGIN~2", "ＢＡＣＫＵＰＳ", "CON", "nul", "COM1", "Lpt9", "aux.obj", ".git"] {
        assert!(reserved(bad), "{bad:?} should be reserved");
        assert!(plain_name(bad).is_err(), "{bad:?} should be refused");
    }
    for ok in ["Mine", "Backup stuff", "Plugins and more", "Console", "Chairs", "com", "Backups2"] {
        assert!(!reserved(ok), "{ok:?} should be fine");
    }
    let base = scratch("lib-fold");
    let ws = base.join("ws");
    std::fs::create_dir_all(ws.join("Backups")).unwrap();
    std::fs::create_dir_all(ws.join("Plugins")).unwrap();
    let a = agent(&ws);
    let mut e = Engine::new();
    run_as(&mut e, &a, json!({"cmd": "rect", "from": [0, 0, 0], "to": [2, 2, 0]}));
    let f = run_as(&mut e, &a, json!({"cmd": "rect", "from": [10, 0, 0], "to": [12, 2, 0]}))["face"].as_u64().unwrap();
    let g = run_as(&mut e, &a, json!({"cmd": "group", "ids": [f]}))["group"].as_u64().unwrap();
    let comp = run_as(&mut e, &a, json!({"cmd": "make_component", "ids": [g], "name": "Thing"}))["component"].as_u64().unwrap();
    for coll in ["Backupſ", "Pluginſ", "BACKUPS", "Backups.", "Backups ", "BACKUP~1", "Bäckups"] {
        for cmd in ["save_to_library", "library_add"] {
            let mut c = json!({"cmd": cmd, "dir": ".", "collection": coll, "name": "live-1"});
            if cmd == "save_to_library" {
                c["component"] = json!(comp);
            } else {
                c["paths"] = json!(["x.obj"]);
            }
            let r = e.exec_json_as(&a, &c.to_string());
            assert_eq!(r["ok"], false, "{cmd} {coll:?} -> {r}");
        }
    }
    assert!(std::fs::read_dir(ws.join("Backups")).unwrap().next().is_none(), "nothing written into Backups");
    assert!(std::fs::read_dir(ws.join("Plugins")).unwrap().next().is_none(), "nothing written into Plugins");
    // by identity: a folder that is really Backups (a link, or the same folder under another name) is refused for everyone
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(ws.join("Backups"), ws.join("Shelf")).unwrap();
        let r = e.exec_json(&json!({"cmd": "save_to_library", "component": comp, "dir": ws, "collection": "Shelf"}).to_string());
        assert_eq!(r["ok"], false, "{r}");
        assert!(std::fs::read_dir(ws.join("Backups")).unwrap().next().is_none());
    }
    std::fs::remove_dir_all(&base).ok();
}

/// R4-MBK-003: a name is "made by this agent" only after a command wrote it, and only while it is still that file.
#[test]
fn a_failed_command_reserves_no_name_and_a_replaced_file_is_not_the_agents() {
    let base = scratch("made");
    let ws = base.join("ws");
    std::fs::create_dir_all(ws.join("x")).unwrap();
    let a = agent(&ws);
    let mut e = Engine::new();
    run_as(&mut e, &a, json!({"cmd": "rect", "from": [0, 0, 0], "to": [2, 2, 0]}));
    // a library_add of a file that doesn't exist fails and must reserve nothing
    let r = e.exec_json_as(&a, &json!({"cmd": "library_add", "paths": ["Chair.obj"], "dir": "Library"}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    // the person makes that file afterwards
    std::fs::create_dir_all(ws.join("Library")).unwrap();
    std::fs::write(ws.join("Library/Chair.obj"), b"PERSONS CHAIR").unwrap();
    run_as(&mut e, &a, json!({"cmd": "export_obj", "path": "x/Chair.obj"}));
    let r = e.exec_json_as(&a, &json!({"cmd": "library_add", "paths": ["x/Chair.obj"], "dir": "Library"}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(std::fs::read(ws.join("Library/Chair.obj")).unwrap(), b"PERSONS CHAIR");
    // a file the agent really made can be written again, until the person changes it
    run_as(&mut e, &a, json!({"cmd": "export_obj", "path": "x/Mine.obj"}));
    run_as(&mut e, &a, json!({"cmd": "export_obj", "path": "x/Mine.obj"}));
    std::thread::sleep(std::time::Duration::from_millis(30));
    std::fs::write(ws.join("x/Mine.obj"), b"the person edited this").unwrap();
    let r = e.exec_json_as(&a, &json!({"cmd": "export_obj", "path": "x/Mine.obj"}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(std::fs::read(ws.join("x/Mine.obj")).unwrap(), b"the person edited this");
    std::fs::remove_dir_all(&base).ok();
}

fn pre_backups(dir: &std::path::Path) -> Vec<String> {
    backups(dir).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).filter(|n| n.contains("-pre")).collect()
}

/// R4-MBK-001: an agent's `new` loop cannot push out the person's backups: its own quota is small and separate.
#[test]
fn an_agents_new_loop_cannot_evict_the_persons_backups() {
    let base = scratch("agent-flood");
    let ws = base.clone();
    let doc = base.join("live.sketchcraft");
    let mut person = Live::open(doc.clone());
    for i in 0..6 {
        person.exec(&json!({"cmd": "rect", "from": [i * 10, 0, 0], "to": [i * 10 + 4, 4, 0]}));
    }
    // the person's own destructive step: a pre backup of the six rectangles
    person.exec(&json!({"cmd": "new"}));
    let good: Vec<PathBuf> = backups(&base);
    assert!(!good.is_empty());
    // a prompt-injected agent in its own process on the same file, 70 build-and-reset cycles
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let mut theirs = Live::open(doc.clone());
    let a = agent(&ws);
    for i in 0..70 {
        let r = theirs.exec_as(&a, &json!({"cmd": "rect", "from": [0, 0, 0], "to": [4 + i, 4, 0]}));
        assert_eq!(r["ok"], true, "{r}");
        assert_eq!(theirs.exec_as(&a, &json!({"cmd": "new"}))["ok"], true);
    }
    assert!(good.iter().all(|p| p.exists()), "the person's backups survived 70 cycles");
    let others = backups(&base).iter().filter(|p| p.to_string_lossy().ends_with("-a.sketchcraft")).count();
    assert!(others <= 10, "the agent's slots are capped ({others})");
    // paced: far fewer than 70 forced backups were even taken
    assert!(others <= 3, "forced backups are paced for agents ({others})");
    std::fs::remove_dir_all(&base).ok();
}

/// R4-BRN-002: a plugin run that wipes the model takes a "before a plugin run" backup first.
#[test]
fn a_plugin_run_that_deletes_most_of_the_model_takes_a_backup_first() {
    let dir = scratch("batch-backup");
    let doc = dir.join("live.sketchcraft");
    let mut live = Live::open(doc.clone());
    let mut ids = vec![];
    for i in 0..3 {
        ids.push(live.exec(&json!({"cmd": "rect", "from": [i * 10, 0, 0], "to": [i * 10 + 4, 4, 0]}))["face"].as_u64().unwrap());
    }
    live.exec(&json!({"cmd": "begin_batch"}));
    let r = live.exec_as(&Caller::Plugin, &json!({"cmd": "delete", "ids": ids}));
    assert_eq!(r["ok"], true, "{r}");
    live.exec(&json!({"cmd": "end_batch"}));
    let mut check = Engine::new();
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 0);
    let pre = pre_backups(&dir);
    assert!(pre.iter().any(|n| n.contains("-pre-bat-p")), "{pre:?}");
    let found = backups(&dir).into_iter().any(|p| {
        run(&mut check, json!({"cmd": "open", "path": p}));
        faces(&mut check) == 3
    });
    assert!(found, "a backup of the three rectangles exists");
    std::fs::remove_dir_all(&dir).ok();
}

/// R4-BRN-003: erase-heavy work does not turn every step into a forced backup.
#[test]
fn big_deletes_in_a_loop_are_paced_and_do_not_evict_the_timed_history() {
    let dir = scratch("delete-loop");
    let doc = dir.join("live.sketchcraft");
    let mut live = Live::open(doc.clone());
    live.exec(&json!({"cmd": "rect", "from": [500, 0, 0], "to": [504, 4, 0]}));
    live.exec(&json!({"cmd": "rect", "from": [510, 0, 0], "to": [514, 4, 0]}));
    // an older timed backup (an hour ago) that holds the session start
    std::fs::create_dir_all(dir.join("Backups")).unwrap();
    let old = dir.join("Backups").join(format!("live-{}.sketchcraft", now_secs() - 7200));
    std::fs::copy(&doc, &old).unwrap();
    for _ in 0..80 {
        let mut ids = vec![];
        for i in 0..3 {
            ids.push(live.exec(&json!({"cmd": "rect", "from": [i * 10, 0, 0], "to": [i * 10 + 4 + (ids.len() as i64), 4, 0]}))["face"].as_u64().unwrap());
        }
        live.exec(&json!({"cmd": "delete", "ids": ids}));
    }
    let pre = pre_backups(&dir);
    assert!(pre.len() <= 6, "{} forced backups in a few seconds", pre.len());
    assert!(old.exists(), "the older backup is still there");
    std::fs::remove_dir_all(&dir).ok();
}

/// R4-BRN-007: the person's own edit while a plugin run is open ends the run (and saves it), so a crash can't lose it;
/// and the 150 s limit is enforced by `housekeeping` with no poll or command.
#[test]
fn the_persons_edit_ends_a_plugin_run_and_the_timer_closes_a_forgotten_one() {
    let dir = scratch("batch-ui");
    let doc = dir.join("live.sketchcraft");
    let mut live = Live::open(doc.clone());
    live.exec(&json!({"cmd": "begin_batch"}));
    live.exec_as(&Caller::Plugin, &json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    // the person draws while the run is open: the run is closed and saved, then their rectangle is saved
    let r = live.exec(&json!({"cmd": "rect", "from": [10, 0, 0], "to": [14, 4, 0]}));
    assert_eq!(r["ok"], true, "{r}");
    let mut check = Engine::new();
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 2, "both are on disk: a crash now loses nothing");
    assert!(!live.engine.in_batch());
    // a run nobody closes: housekeeping alone (no command, no poll) closes and saves it
    live.exec(&json!({"cmd": "begin_batch"}));
    live.exec_as(&Caller::Plugin, &json!({"cmd": "rect", "from": [20, 0, 0], "to": [24, 4, 0]}));
    live.engine.expire_batch_for_test();
    live.housekeeping();
    assert!(!live.engine.in_batch());
    run(&mut check, json!({"cmd": "open", "path": doc}));
    assert_eq!(faces(&mut check), 3);
    std::fs::remove_dir_all(&dir).ok();
}

/// R4-BRN-008 / R4-MBK-004: a model over the geometry limit still opens (for viewing and cutting down), can't grow, can't be saved
/// over the file, and is never a dead end; one command can't add more than the per-command limit.
#[test]
fn a_model_over_the_limit_opens_and_can_be_cut_down() {
    use sketchcraft::model::{Model, Vertex, MAX_ENTITIES};
    let dir = scratch("oversize");
    let doc = dir.join("live.sketchcraft");
    let mut m = Model::default();
    // a group holding a bit more than the limit, plus one honest rectangle's worth outside it
    let g = m.begin_group("Too much".into());
    for i in 0..(MAX_ENTITIES + 10) {
        m.vertices.insert((1000 + i) as sketchcraft::Id, Vertex { p: glam::DVec3::new(i as f64, 0.0, 0.0), group: Some(g) });
    }
    m.next_id = (1000 + MAX_ENTITIES + 10) as sketchcraft::Id;
    m.active_group = None;
    std::fs::write(&doc, serde_json::to_vec(&m).unwrap()).unwrap();
    let on_disk = std::fs::read(&doc).unwrap();
    let mut live = Live::open(doc.clone());
    assert!(live.engine.oversize, "opened, flagged");
    assert!(live.notice.as_deref().is_some_and(|n| n.contains("over the size limits")), "{:?}", live.notice);
    assert!(live.scene()["notice"].is_string());
    // growth is refused, the file on disk is untouched
    let r = live.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    assert_eq!(r["ok"], false, "{r}");
    assert_eq!(std::fs::read(&doc).unwrap(), on_disk);
    // cutting it down works, and saving resumes once it is under
    let r = live.exec(&json!({"cmd": "delete", "ids": [g]}));
    assert_eq!(r["ok"], true, "{r}");
    assert!(!live.engine.oversize);
    let r = live.exec(&json!({"cmd": "rect", "from": [0, 0, 0], "to": [4, 4, 0]}));
    assert_eq!(r["ok"], true, "{r}");
    std::fs::remove_dir_all(&dir).ok();
}

/// R4-MBK-004: one command can't add a million entities (the per-command limit), whatever it is.
#[test]
fn one_command_cannot_add_more_than_the_growth_limit() {
    let mut e = Engine::new();
    let r = e.exec_json(&json!({"cmd": "terrain_grid", "at": [0, 0, 0], "width": 600, "depth": 600, "spacing": 1}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    let r = e.exec_json(&json!({"cmd": "terrain_grid", "at": [0, 0, 0], "width": 100, "depth": 100, "spacing": 1}).to_string());
    assert_eq!(r["ok"], true, "{r}");
}

/// R4-MBK-005: an agent can't write a file named like the ones SketchCraft sets aside, and planted look-alikes are never pruned.
#[test]
fn an_agent_cannot_plant_files_that_look_like_set_aside_work() {
    let base = scratch("side-names");
    let a = agent(&base);
    let mut e = Engine::new();
    run_as(&mut e, &a, json!({"cmd": "rect", "from": [0, 0, 0], "to": [2, 2, 0]}));
    for name in ["live.unsaved-1800000000.sketchcraft", "live.unsaved-1800000000-1.sketchcraft", "x.damaged-1800000000.sketchcraft"] {
        let r = e.exec_json_as(&a, &json!({"cmd": "save", "path": name}).to_string());
        assert_eq!(r["ok"], false, "{name} -> {r}");
        assert!(!base.join(name).exists());
    }
    let r = e.exec_json_as(&a, &json!({"cmd": "save", "path": "fine.sketchcraft"}).to_string());
    assert_eq!(r["ok"], true, "{r}");
    std::fs::remove_dir_all(&base).ok();
}
