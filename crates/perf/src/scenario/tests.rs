use super::*;

fn sample() -> Scenario {
    Scenario {
        id: "t".into(),
        about: "a test".into(),
        source: Source::NewProject { mesh: PathBuf::from("/m.obj"), resolution: 4096 },
        action: Action::Stroke(StrokeSpec {
            tool: StrokeTool::Erase,
            size: Some(0.25),
            spacing: Some(0.5),
            frame_ms: Some(16.0),
            target: StrokeTarget::Mask,
            symmetry: Symmetry::Radial { axis: 1, count: 6 },
            path: PathShape::Spiral,
            primed: true,
            ..StrokeSpec::default()
        }),
        repeat: 3,
    }
}

#[test]
fn round_trips_through_json() {
    let s = sample();
    let text = s.to_json().to_string();
    let back = Scenario::from_json(&materialcraft::json::parse(&text).unwrap()).unwrap();
    assert_eq!(back, s);
    let edits = Scenario { action: Action::Edits(EditKind::ALL.to_vec()), source: Source::Document("/d.mcpaint".into()), ..s };
    assert_eq!(Scenario::from_json(&edits.to_json()).unwrap(), edits);
}

#[test]
fn missing_fields_take_defaults() {
    let v = materialcraft::json::parse(r#"{"id": "x", "source": {"mesh": "/m.obj"}, "action": {"stroke": {}}}"#).unwrap();
    let s = Scenario::from_json(&v).unwrap();
    assert_eq!(s.repeat, 1);
    assert_eq!(s.source, Source::NewProject { mesh: "/m.obj".into(), resolution: 2048 });
    assert_eq!(s.action, Action::Stroke(StrokeSpec::default()));
}

#[test]
fn rejects_typos_and_bad_values() {
    for bad in [
        r#"{"id": "x", "source": {"mesh": "/m.obj"}, "action": {"stroke": {"tol": "paint"}}}"#,
        r#"{"id": "x", "source": {"mesh": "/m.obj"}, "action": {"stroke": {"tool": "pencil"}}}"#,
        r#"{"id": "x", "source": {"mesh": "/m.obj"}, "action": {"stroke": {"spacing": 0}}}"#,
        r#"{"id": "x", "source": {"mesh": "/m.obj"}, "action": {"stroke": {"frame_ms": -16}}}"#,
        r#"{"id": "x", "source": {"mesh": "/m.obj", "resolution": 1000}, "action": "open"}"#,
        r#"{"id": "x", "source": {"document": "/d"}, "action": {"edits": ["opacty"]}}"#,
        r#"{"id": "x", "source": {"document": "/d"}, "action": "open", "repeat": 0}"#,
        r#"{"source": {"document": "/d"}, "action": "open"}"#,
    ] {
        assert!(Scenario::from_json(&materialcraft::json::parse(bad).unwrap()).is_err(), "{bad}");
    }
}
