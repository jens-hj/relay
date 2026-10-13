use super::*;
use crate::prelude::*;
use crate::testing::mount;
use mosaic::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

type Log<T> = Rc<RefCell<Vec<T>>>;

fn recorder<T: 'static>() -> (Log<T>, Log<T>) {
    let log = Rc::new(RefCell::new(Vec::new()));
    (log.clone(), log)
}

#[test]
fn connection_cell_states_and_popover() {
    assert_eq!(latency_bars(20), 4);
    assert_eq!(latency_bars(120), 3);
    assert_eq!(latency_bars(300), 2);
    assert_eq!(latency_bars(900), 1);
    let state = State::new(Connection::Connected);
    let latency = State::new(Some(42u32));
    let mounted = mount(Size::new(400.0, 400.0), move || {
        let info = ConnectionInfo {
            endpoint: "relay.local:7171".into(),
            transport: "WebSocket".into(),
            protocol: "5".into(),
            revision: String::new(),
        };
        view! {
            col justify:end {
                ConnectionStatus state:(state) latency:(latency) info:(info)
            }
        }
    });
    assert!(mounted.has("Connected, 42 ms"));
    assert_eq!(mounted.rect("Connected, 42 ms").size.height, 40.0);
    mounted.click_label("Connected, 42 ms");
    assert!(mounted.has("relay.local:7171"));
    assert!(mounted.has("WebSocket"));
    assert!(!mounted.has("Revision"), "empty fields are hidden");
    mounted.key(Key::Escape);
    state.set(Connection::Offline);
    latency.set(None);
    mounted.settle();
    assert!(mounted.has("Offline"));
}

#[test]
fn palette_filters_and_runs_with_keys() {
    let commands = vec![
        Command::new("new", "New agent").keys("N"),
        Command::new("stop", "Stop agent").keys("⇧S"),
        Command::new("theme", "Switch theme"),
    ];
    assert_eq!(filter_commands(&commands, "AGENT").len(), 2);
    assert_eq!(filter_commands(&commands, "stop ag").len(), 1);
    assert_eq!(filter_commands(&commands, "").len(), 3);
    let (ran, log) = recorder::<String>();
    let query = State::new(String::new());
    let mounted = mount(Size::new(700.0, 400.0), move || {
        let run: OnCommand = Rc::new(move |id| log.borrow_mut().push(id));
        view! {
            col {
                CommandPalette commands:(commands) query:(query) on-run:(run)
            }
        }
    });
    assert!(mounted.has("Switch theme"));
    query.set("agent".into());
    mounted.settle();
    assert!(!mounted.has("Switch theme"));
    mounted.click_label("Command");
    mounted.key(Key::ArrowDown);
    mounted.key(Key::Enter);
    assert_eq!(ran.borrow().as_slice(), ["stop"]);
    mounted.click_label("New agent");
    assert_eq!(ran.borrow().last().map(String::as_str), Some("new"));
    query.set("zzz".into());
    mounted.settle();
    assert!(mounted.has("No matching commands"));
}

#[test]
fn form_controls_report_choices() {
    let (picked, pick_log) = recorder::<usize>();
    let (numbers, number_log) = recorder::<i64>();
    let (switches, switch_log) = recorder::<bool>();
    let (selects, select_log) = recorder::<usize>();
    let value = State::new(String::new());
    let mounted = mount(Size::new(800.0, 600.0), move || {
        let pick: OnIndex = Rc::new(move |i| pick_log.borrow_mut().push(i));
        let number: OnNumber = Rc::new(move |n| number_log.borrow_mut().push(n));
        let switch: OnSwitch = Rc::new(move |b| switch_log.borrow_mut().push(b));
        let select: OnIndex = Rc::new(move |i| select_log.borrow_mut().push(i));
        let options = vec!["Dark".to_string(), "Light".to_string()];
        view! {
            col gap:8px {
                Field label:"Repository" value:(value) placeholder:"owner/name"
                Segments options:(options.clone()) selected:1usize on-select:(pick)
                Stepper value:9i64 on-change:(number) min:0i64 max:10i64 step:2i64 label:"Workers"
                Toggle label:"Compact rows" on:false on-toggle:(switch)
                Select label:"Theme" options:(options) selected:0usize on-select:(select)
            }
        }
    });
    assert!(mounted.has("Repository"));
    assert_eq!(mounted.rect("Repository").size.height, 40.0);
    mounted.click_label("Dark");
    assert_eq!(picked.borrow().as_slice(), [0]);
    assert!(mounted.has("Workers"));
    mounted.click_label("Increase");
    mounted.click_label("Decrease");
    assert_eq!(numbers.borrow().as_slice(), [10, 7], "clamped to max");
    mounted.click_label("Compact rows");
    assert_eq!(switches.borrow().as_slice(), [true]);
    mounted.click_label("Theme");
    let light = mounted.rects("Light");
    mounted.click(light.last().unwrap().center());
    assert_eq!(selects.borrow().as_slice(), [1]);
}

#[test]
fn board_surfaces_carry_labels_and_callbacks() {
    let (changes, change_log) = recorder::<(usize, Permission)>();
    let (opened, open_log) = recorder::<&'static str>();
    let mounted = mount(Size::new(1200.0, 700.0), move || {
        let change: OnPermission =
            Rc::new(move |row, choice| change_log.borrow_mut().push((row, choice)));
        let open: OnPress = Rc::new(move || open_log.borrow_mut().push("issue"));
        let close: OnPress = Rc::new(|| {});
        let issue = IssueView {
            identifier: "#31".into(),
            title: "Agent tree".into(),
            status: Some(AgentStatus::Working),
            labels: vec!["ui".into()],
            selected: false,
            ..Default::default()
        };
        let rows = vec![
            PermissionRow {
                action: "Write to the database".into(),
                choice: Permission::Ask,
                pending: true,
                name: String::new(),
                flags: vec![],
            },
            PermissionRow {
                action: "Run tests".into(),
                choice: Permission::Allow,
                pending: false,
                name: String::new(),
                flags: vec![],
            },
        ];
        view! {
            row {
                BoardColumn name:"In progress" count:1usize {
                    IssueCard issue:(issue) on-open:(Some(open))
                }
                col width:{400.0}px {
                    PermissionMatrix entries:(rows) on-change:(change)
                    HarnessCard harness:(Harness::Codex) version:"0.42.0"
                        auth:(HarnessAuth::SignedIn)
                    Notice text:"Push rejected" tone:(NoticeTone::Failure)
                        actions:(vec![("Retry".to_string(), Rc::new(|| {}) as OnPress)])
                }
                IssueInspector identifier:"#31" title:"Agent tree" on-close:(Some(close))
            }
        }
    });
    assert_eq!(mounted.rect("In progress").size.width, 300.0);
    assert!(
        mounted
            .rects("Agent tree")
            .iter()
            .any(|rect| rect.size.width == 392.0),
        "the inspector is 392px"
    );
    mounted.click(mounted.rects("Agent tree")[0].center());
    assert_eq!(opened.borrow().len(), 1);
    assert_eq!(mounted.count("deny"), 2, "one visible word per row");
    mounted.click(mounted.rect("Write to the database: deny").center());
    assert!(mounted.has("Run tests: allow"));
    assert_eq!(changes.borrow().as_slice(), [(0, Permission::Deny)]);
    for label in [
        "Codex",
        "0.42.0",
        "Signed in",
        "Push rejected",
        "Retry",
        "Close",
    ] {
        assert!(mounted.has(label), "{label}");
    }
}

#[test]
fn page_header_cells_sit_flush_right() {
    let mounted = mount(Size::new(900.0, 200.0), || {
        let press: OnPress = Rc::new(|| {});
        view! {
            col {
                PageHeader title:"Settings" path:(vec!["Relay".to_string()]) {
                    HeaderCell label:"Save" on-press:(press) keys:(Some("⌘S".to_string()))
                }
                Module title:"Harnesses" caps:true {
                    Label text:"Body"
                }
            }
        }
    });
    let save = mounted.rect("Save");
    assert_eq!(save.origin.x + save.size.width, 900.0);
    assert_eq!(save.size.height, 72.0);
    assert!(mounted.has("Relay /"));
    assert!(mounted.has("HARNESSES"));
}

#[test]
fn surfaces_take_the_slots_and_names_applications_need() {
    let flags = Rc::new(RefCell::new(Vec::new()));
    let flag_log = flags.clone();
    let closed = Rc::new(RefCell::new(0));
    let close_log = closed.clone();
    let query = State::new(String::new());
    let value = State::new("draft".to_string());
    let mounted = mount(Size::new(1400.0, 1400.0), move || {
        let on_flag: OnFlag =
            Rc::new(move |row, column, on| flag_log.borrow_mut().push((row, column, on)));
        let close: OnPress = Rc::new(move || *close_log.borrow_mut() += 1);
        let noop: OnPress = Rc::new(|| {});
        let on_change: Rc<dyn Fn(usize, Permission)> = Rc::new(|_, _| {});
        let on_run: Rc<dyn Fn(String)> = Rc::new(|_| {});
        let on_step: OnNumber = Rc::new(|_| {});
        let issue = IssueView {
            identifier: "#2".into(),
            title: "Profiles".into(),
            preview: "Define defaults".into(),
            meta: "2 sessions".into(),
            label: "Open issue #2".into(),
            description: "Working · 2 linked sessions".into(),
            ..Default::default()
        };
        let rows = vec![PermissionRow {
            action: "Plan".into(),
            choice: Permission::Allow,
            pending: false,
            name: "Plan permission".into(),
            flags: vec![true, false],
        }];
        let columns = vec![
            FlagColumn {
                head: "Resp.".into(),
                name: "responsibility".into(),
            },
            FlagColumn {
                head: "Done".into(),
                name: "required for completion".into(),
            },
        ];
        let commands = vec![Command::new("board", "Open board").detail("Navigate")];
        let spacer = view! {
            el width:20px height:20px label:"End" {}
        };
        let footer = view! {
            row height:40px label:"Footer" {}
        };
        let tail = view! {
            el width:20px height:20px label:"Head end" {}
        };
        view! {
            col {
                PageHeader title:"Board" eyebrow:(Some("Project board".into())) height:(60.0) {
                    ReadoutCell key:"Source" value:"Local board"
                }
                Module title:"Harnesses" caps:true note:(Some("On the server".into()))
                    head-end:(Some(tail))
                BoardColumn name:"Backlog" count:2usize fill:true
                    head-label:("Column backlog".to_string()) {
                    IssueCard issue:(issue)
                }
                IssueInspector identifier:"#2" title:"Profiles" on-close:(Some(close.clone()))
                    close-label:"Close issue details" head-end:(Some(spacer)) footer:(Some(footer))
                PermissionMatrix entries:(rows) on-change:(on_change) flag-columns:(columns)
                    on-flag:(Some(on_flag))
                HarnessCard harness:(Harness::Codex)
                    status:(Some((AgentStatus::Done, "Ready".to_string())))
                    actions:(vec![("Refresh".to_string(), noop.clone())])
                Notice text:"Saved" detail:(Some("Request r12".into()))
                    actions:(vec![("Retry original request".to_string(), noop.clone())])
                CommandPalette commands:(commands) query:(query) on-run:(on_run)
                    title:(Some("Command palette".to_string())) on-close:(Some(close))
                    close-label:"Close command palette" field-label:"Command search" footer:true
                TextInput value:(value) label:"Worker prompt" multiline:true
                Stepper value:100i64 on-change:(on_step) label:"Interface scale percent"
                    decrease-label:"Decrease interface scale"
                    increase-label:"Increase interface scale" suffix:"%"
            }
        }
    });
    for label in [
        "Project board",
        "Source",
        "Local board",
        "On the server",
        "Head end",
        "Column backlog",
        "Open issue #2",
        "Preview",
        "2 sessions",
        "Close issue details",
        "End",
        "Footer",
        "Plan responsibility",
        "Plan required for completion",
        "Plan permission: allow",
        "Ready",
        "Refresh",
        "Request r12",
        "Retry original request",
        "Close command palette",
        "Command search",
        "Command results",
        "Navigate",
        "1 command",
        "Worker prompt",
        "Interface scale percent",
        "Decrease interface scale",
    ] {
        assert!(mounted.has(label), "{label}: {:?}", mounted.labels());
    }
    mounted.click_label("Plan required for completion");
    assert_eq!(flags.borrow().as_slice(), [(0, 1, true)]);
    mounted.click_label("Close issue details");
    mounted.click_label("Close command palette");
    assert_eq!(*closed.borrow(), 2);
    assert_eq!(mounted.rect("Column backlog").size.height, 40.0);
}

#[test]
fn harness_choices_move_the_choice_and_focus_with_arrow_keys() {
    let chosen = State::new(0usize);
    let mounted = mount(Size::new(800.0, 300.0), move || {
        let pick: OnIndex = Rc::new(move |index| chosen.set(index));
        let options = [Harness::Codex, Harness::ClaudeCode]
            .into_iter()
            .map(|harness| HarnessOption {
                harness,
                label: format!("Agent harness: {}", harness.name()),
                description: "Ready".into(),
                status: Some((AgentStatus::Done, "Ready".into())),
            })
            .collect::<Vec<_>>();
        view! {
            col {
                HarnessChoices options:(options) selected:(Live::from(chosen)) on-select:(pick)
            }
        }
    });
    let codex = mounted.rect("Agent harness: Codex");
    let claude = mounted.rect("Agent harness: Claude Code");
    assert!(
        claude.origin.x >= codex.origin.x + codex.size.width - 1.0,
        "side by side"
    );
    mounted.click_label("Agent harness: Claude Code");
    assert_eq!(chosen.get_untracked(), 1);
    mounted.key(Key::Home);
    assert_eq!(chosen.get_untracked(), 0);
    assert_eq!(
        mounted.focused_label().as_deref(),
        Some("Agent harness: Codex")
    );
    mounted.key(Key::ArrowRight);
    assert_eq!(chosen.get_untracked(), 1);
    assert_eq!(
        mounted.focused_label().as_deref(),
        Some("Agent harness: Claude Code")
    );
}

#[test]
fn live_permission_rows_keep_focus_while_they_change() {
    let rows = State::new(vec![PermissionRow {
        action: "Merge".into(),
        choice: Permission::Deny,
        pending: false,
        name: "Merge permission".into(),
        flags: vec![false],
    }]);
    let mounted = mount(Size::new(800.0, 300.0), move || {
        let change: OnPermission = Rc::new(move |_, choice| {
            rows.update(|rows| rows[0].choice = choice);
        });
        let flag: OnFlag = Rc::new(move |_, _, on| rows.update(|rows| rows[0].flags[0] = on));
        let columns = vec![FlagColumn {
            head: "Done".into(),
            name: "required".into(),
        }];
        let choices = vec![
            (Permission::Deny, "Deny".to_string()),
            (Permission::Ask, "Ask".to_string()),
            (Permission::Allow, "Allow".to_string()),
        ];
        view! {
            col {
                PermissionMatrix entries:(Live::from(rows)) on-change:(change)
                    flag-columns:(columns) on-flag:(Some(flag)) choices:(choices)
            }
        }
    });
    mounted.click_label("Merge permission: Allow");
    assert_eq!(rows.get_untracked()[0].choice, Permission::Allow);
    assert_eq!(
        mounted.focused_label().as_deref(),
        Some("Merge permission: Allow")
    );
    mounted.key(Key::ArrowLeft);
    assert_eq!(rows.get_untracked()[0].choice, Permission::Ask);
    mounted.click_label("Merge required");
    mounted.click_label("Merge required");
    assert!(
        !rows.get_untracked()[0].flags[0],
        "each press flips the current value"
    );
}

#[test]
fn a_swipe_across_an_issue_card_does_not_open_it() {
    let opened = Rc::new(RefCell::new(0));
    let count = opened.clone();
    let mounted = mount(Size::new(400.0, 200.0), move || {
        let open: OnPress = Rc::new(move || *count.borrow_mut() += 1);
        let issue = IssueView {
            identifier: "#1".into(),
            title: "Card".into(),
            ..Default::default()
        };
        view! {
            col {
                IssueCard issue:(issue) on-open:(Some(open))
            }
        }
    });
    let card = mounted.rect("Card");
    let y = card.center().y;
    for (kind, x) in [
        (PointerEventKind::Down(PointerButton::Primary), 40.0),
        (PointerEventKind::Move, 220.0),
        (PointerEventKind::Up(PointerButton::Primary), 220.0),
    ] {
        mounted.ui.dispatch_pointer(PointerEvent {
            kind,
            position: Vector2::new(x, y),
            pointer_type: PointerType::Touch,
            modifiers: Modifiers::default(),
            timestamp: std::time::Duration::ZERO,
        });
        mounted.settle();
    }
    assert_eq!(*opened.borrow(), 0);
    mounted.click_label("Card");
    assert_eq!(*opened.borrow(), 1);
}
