//! Final pre-0.3.6 pass: built-in assets and library parts are found by identity, not by a name
//! anyone can give a component (SPECTER-001), and a model file never carries a folder path (CASHOUT-004).

use serde_json::{json, Value};
use sketchcraft::Engine;

fn ok(e: &mut Engine, v: Value) -> Value {
    let r = e.exec_json(&v.to_string());
    assert_eq!(r["ok"], true, "{v} -> {r}");
    r
}

#[test]
fn a_component_named_door_does_not_take_over_the_built_in_door() {
    let mut e = Engine::new();
    let face = ok(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [2, 2, 0]}))["face"].clone();
    let planted = ok(&mut e, json!({"cmd": "make_component", "ids": [face], "name": "Door"}))["component"].clone();
    let placed = ok(&mut e, json!({"cmd": "insert", "asset": "door", "at": [10, 0, 0]}));
    assert_ne!(placed["component"], planted, "{placed}");
    // the built-in is reused the second time
    let again = ok(&mut e, json!({"cmd": "insert", "asset": "door", "at": [20, 0, 0]}));
    assert_eq!(again["component"], placed["component"]);
    // and a component that claims the built-in's reserved key but has other geometry isn't reused either
    let key = e.model.definitions[&placed["component"].as_u64().unwrap().try_into().unwrap()].source.clone();
    assert_eq!(key.as_deref(), Some("builtin:door"));
    let pid: u32 = planted.as_u64().unwrap().try_into().unwrap();
    e.model.definitions.get_mut(&pid).unwrap().source = key;
    e.model.definitions.get_mut(&pid).unwrap().cuts = true;
    let third = ok(&mut e, json!({"cmd": "insert", "asset": "door", "at": [30, 0, 0]}));
    assert_eq!(third["component"], placed["component"]);
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("sketchcraft-final-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn part(path: &std::path::Path, size: f64) {
    let mut e = Engine::new();
    let face = ok(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [size, size, 0]}))["face"].clone();
    ok(&mut e, json!({"cmd": "make_component", "ids": [face], "name": "Chair"}));
    let m = sketchcraft::library::standalone(&e.model, *e.model.definitions.keys().next().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string(&m).unwrap()).unwrap();
}

#[test]
fn library_parts_are_reused_by_content_and_the_saved_model_holds_no_folder_path() {
    let dir = scratch("lib");
    let (a, b) = (dir.join("one").join("chair.sketchcraft"), dir.join("two").join("chair.sketchcraft"));
    std::fs::create_dir_all(a.parent().unwrap()).unwrap();
    std::fs::create_dir_all(b.parent().unwrap()).unwrap();
    part(&a, 1.0);
    part(&b, 2.0);
    let mut e = Engine::new();
    let p1 = ok(&mut e, json!({"cmd": "insert_file", "path": a.to_string_lossy(), "at": [0, 0, 0]}));
    let p2 = ok(&mut e, json!({"cmd": "insert_file", "path": a.to_string_lossy(), "at": [5, 0, 0]}));
    let p3 = ok(&mut e, json!({"cmd": "insert_file", "path": b.to_string_lossy(), "at": [9, 0, 0]}));
    assert_eq!(p1["component"], p2["component"], "the same file is reused");
    assert_ne!(p1["component"], p3["component"], "same name, other contents: its own component");
    let saved = serde_json::to_string(&e.model).unwrap();
    assert!(!saved.contains(dir.to_string_lossy().as_ref()), "a folder path is in the model file");
    assert!(!saved.contains("sketchcraft-final-lib"), "a folder name is in the model file");
    // the scene an agent sees names no folder either
    let scene = ok(&mut e, json!({"cmd": "components"})).to_string();
    assert!(!scene.contains("sketchcraft-final-lib"), "{scene}");
    // a changed file is a new component
    part(&a, 3.0);
    let p4 = ok(&mut e, json!({"cmd": "insert_file", "path": a.to_string_lossy(), "at": [0, 5, 0]}));
    assert_ne!(p4["component"], p1["component"]);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_older_model_file_with_an_absolute_source_loads_and_loses_the_folder() {
    let mut e = Engine::new();
    let face = ok(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [2, 2, 0]}))["face"].clone();
    ok(&mut e, json!({"cmd": "make_component", "ids": [face], "name": "Chair"}));
    let mut v: Value = serde_json::to_value(&e.model).unwrap();
    for d in v["definitions"].as_object_mut().unwrap().values_mut() {
        d["source"] = json!("/Users/someone/Documents/SketchCraft/Library/chair.sketchcraft");
    }
    let old = serde_json::to_string(&v).unwrap();
    let m: sketchcraft::model::Model = serde_json::from_str(&old).unwrap();
    let src = m.definitions.values().next().unwrap().source.clone().unwrap();
    assert_eq!(src, "chair.sketchcraft");
    let win: sketchcraft::model::Model = serde_json::from_str(&old.replace("/Users/someone/Documents/SketchCraft/Library/", "\\\\\\\\?\\\\C:\\\\Users\\\\x\\\\")).unwrap();
    assert_eq!(win.definitions.values().next().unwrap().source.as_deref(), Some("x\\\\chair.sketchcraft").map(|_| "chair.sketchcraft"));
}

fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
    use std::io::Write;
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let opts: zip::write::SimpleFileOptions = zip::write::SimpleFileOptions::default();
        for (name, body) in files {
            z.start_file(*name, opts).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
    }
    buf.into_inner()
}

fn b64(b: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(b)
}

/// SPECTER-002: what the preview calls the plugin is what the installer makes of it.
#[test]
fn a_plugin_that_says_ruby_is_previewed_and_installed_as_ruby() {
    let home = scratch("plug");
    std::env::set_var("SKETCHCRAFT_DOC", home.join("live.sketchcraft"));
    let mut e = Engine::new();
    let rb = "UI.menu('Plugins').add_item(\"Tidy edges\") { }\nUI::Command.new('Other thing') { }\n";
    let sneaky = zip_of(&[
        ("plugin.json", r#"{"name":"Tidy Edges","commands":[{"label":"Harmless"}],"ruby":true,"entry":["loader.rb"]}"#),
        ("loader.rb", rb),
    ]);
    let p = ok(&mut e, json!({"cmd": "install_plugin", "data": b64(&sneaky), "preview": true}));
    assert_eq!(p["ruby"], true, "{p}");
    assert_eq!(p["kind"], "SketchUp Ruby extension");
    assert_eq!(p["commands"], json!(["Tidy edges", "Other thing"]), "the menu items come from the Ruby code, not the manifest");
    let r = ok(&mut e, json!({"cmd": "install_plugin", "data": b64(&sneaky)}));
    assert_eq!(r["ruby"], true);
    assert_eq!(r["sha256"], p["sha256"]);
    let list = ok(&mut e, json!({"cmd": "plugins"}));
    let me = list["plugins"].as_array().unwrap().iter().find(|x| x["id"] == "tidy-edges").unwrap();
    assert_eq!(me["ruby"], true);
    // both at once is refused
    let both = zip_of(&[("plugin.json", r#"{"name":"Mixed","commands":[],"ruby":true}"#), ("main.js", "1"), ("a.rb", "puts 1")]);
    let r = e.exec_json(&json!({"cmd": "install_plugin", "data": b64(&both), "preview": true}).to_string());
    assert_eq!(r["ok"], false, "{r}");
    // a plain JavaScript plugin is still one
    let js = zip_of(&[("plugin.json", r#"{"name":"Plain","commands":[{"label":"Go"}]}"#), ("main.js", "1")]);
    let p = ok(&mut e, json!({"cmd": "install_plugin", "data": b64(&js), "preview": true}));
    assert_eq!(p["ruby"], false);
    assert_eq!(p["kind"], "JavaScript plugin");
    std::fs::remove_dir_all(&home).ok();
}

/// SPECTER-005 and SPECTER-003: agents don't start programs; `--any-path` still keeps the reserved folders.
#[test]
fn agents_cannot_start_programs_and_any_path_keeps_the_reserved_folders() {
    use sketchcraft::policy::{Caller, Workspace};
    let mut e = Engine::new();
    let agent = Caller::Mcp(Workspace::anywhere());
    let dir = scratch("anypath");
    for cmd in ["animate", "import_ifc", "import_dwg", "export_dwg"] {
        let r = e.exec_json_as(&agent, &json!({"cmd": cmd, "path": dir.join("x.mp4").to_string_lossy()}).to_string());
        assert_eq!(r["ok"], false, "{cmd}: {r}");
        assert!(r["error"].as_str().unwrap().contains("isn't available to agents"), "{r}");
    }
    std::fs::create_dir_all(dir.join("Backups")).unwrap();
    std::fs::create_dir_all(dir.join("Plugins")).unwrap();
    std::fs::create_dir_all(dir.join(".secret")).unwrap();
    for bad in ["Backups/live-1.sketchcraft", "Plugins/x.sketchcraft", ".secret/x.sketchcraft", "backups/x.sketchcraft", "live.unsaved-5.sketchcraft"] {
        let p = dir.join(bad);
        let r = e.exec_json_as(&agent, &json!({"cmd": "save", "path": p.to_string_lossy()}).to_string());
        assert_eq!(r["ok"], false, "{bad}: {r}");
        assert!(!p.exists());
    }
    let good = dir.join("fine.sketchcraft");
    let r = e.exec_json_as(&agent, &json!({"cmd": "save", "path": good.to_string_lossy()}).to_string());
    assert_eq!(r["ok"], true, "{r}");
    std::fs::remove_dir_all(&dir).ok();
}

/// SPECTER-006: a plugin run says who made it and how much it changed; Undo names whose step it takes back.
#[test]
fn undo_steps_carry_who_made_them_and_a_plugin_run_reports_what_it_changed() {
    use sketchcraft::policy::Caller;
    let mut e = Engine::new();
    ok(&mut e, json!({"cmd": "rect", "from": [0, 0, 0], "to": [1, 1, 0]}));
    assert_eq!(e.undo_origin(), Some("you"));
    // a plugin run, as the app does it: a batch around the plugin's commands
    ok(&mut e, json!({"cmd": "begin_batch"}));
    for i in 0..3 {
        let r = e.exec_value_as_from(&Caller::Plugin, json!({"cmd": "rect", "from": [5 + i * 3, 0, 0], "to": [6 + i * 3, 1, 0]}), Some("Cut list"));
        assert_eq!(r["ok"], true, "{r}");
    }
    let end = ok(&mut e, json!({"cmd": "end_batch"}));
    assert_eq!(end["origin"], "plugin “Cut list”", "{end}");
    assert!(end["changed"].as_u64().unwrap() >= 3, "{end}");
    assert_eq!(e.undo_origin(), Some("plugin “Cut list”"));
    // the person's own step after it is theirs
    ok(&mut e, json!({"cmd": "rect", "from": [0, 5, 0], "to": [1, 6, 0]}));
    assert_eq!(e.undo_origin(), Some("you"));
    let u = ok(&mut e, json!({"cmd": "undo"}));
    assert_eq!(u["origin"], "you");
    assert_eq!(e.undo_origin(), Some("plugin “Cut list”"));
    assert_eq!(e.redo_origin(), Some("you"));
    let u = ok(&mut e, json!({"cmd": "undo"}));
    assert_eq!(u["origin"], "plugin “Cut list”");
    // a plugin that changed nothing says nothing
    ok(&mut e, json!({"cmd": "begin_batch"}));
    let end = ok(&mut e, json!({"cmd": "end_batch"}));
    assert!(end.get("changed").is_none(), "{end}");
    // a plugin can't claim to be the person: its own "as"/"origin" keys mean nothing
    let r = e.exec_value_as_from(&Caller::Plugin, json!({"cmd": "rect", "from": [9, 9, 0], "to": [10, 10, 0], "origin": "you"}), None);
    assert!(r["ok"] == true || r["ok"] == false);
    assert_ne!(e.undo_origin(), Some("you"));
}

/// SPECTER-006: what an agent saved while the app was closed is announced when the person's window opens.
#[test]
fn the_person_is_told_what_an_agent_changed_while_the_app_was_closed() {
    use sketchcraft::live::Live;
    use sketchcraft::policy::{Caller, Workspace};
    let dir = scratch("agent");
    let doc = dir.join("live.sketchcraft");
    // the agent's own server (no person's window): it saves, and tells no one yet
    let mut agent = Live::open(doc.clone());
    let ws = Caller::Mcp(Workspace::at(&dir));
    let r = agent.exec_as(&ws, &json!({"cmd": "rect", "from": [0, 0, 0], "to": [2, 2, 0]}));
    assert_eq!(r["ok"], true, "{r}");
    assert!(agent.scene()["notice"].is_null(), "{}", agent.scene());
    drop(agent);
    // the person opens the app
    let mut app = Live::open(doc.clone());
    app.as_person_view();
    let note = app.scene()["notice"].as_str().unwrap_or("").to_string();
    assert!(note.starts_with("Agent changed this model"), "{note}");
    // said once
    let mut again = Live::open(doc);
    again.as_person_view();
    assert!(again.scene()["notice"].is_null());
    std::fs::remove_dir_all(&dir).ok();
}
