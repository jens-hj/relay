//! D2 board specimens. Example values live here, never in library defaults.
use mosaic::prelude::*;
use relay_ui::prelude::*;
use relay_ui::tokens::{ToolKind, ground, px};
use std::rc::Rc;
fn noop() -> OnPress {
    Rc::new(|| {})
}
pub fn model() -> ModelIdentity {
    ModelIdentity {
        family: "Sol".into(),
        version: "6.1".into(),
        vendor: "GPT".into(),
        hue: ModelHue::Sol,
    }
}
pub fn effort() -> Effort {
    Effort {
        levels: 5,
        current: 4,
        has_max: false,
        at_max: false,
    }
}
pub fn settings(collapsed: bool) -> LedgerView {
    LedgerView {
        harness: Harness::Codex,
        model: model(),
        effort: effort(),
        collapsed,
    }
}
fn code(lines: &[&str]) -> Vec<CodeLine> {
    lines
        .iter()
        .enumerate()
        .map(|(i, s)| CodeLine {
            number: Some(i as u32 + 1),
            text: (*s).into(),
            ..Default::default()
        })
        .collect()
}
pub fn specimens() -> Vec<ToolCallView> {
    use ToolDetail::*;
    let calls = vec![
        (ToolKind::Read, "Read", "src/db/schema.ts", Read { start:Some(1), end:Some(120), total:Some(342), found:Some("users, orgs, invoices, payments, sessions, audit_log".into()), lines:Some(code(&["import { pgTable, uuid, text } from 'drizzle-orm/pg-core';", "", "export const users = pgTable('users', {", "  id: uuid('id').primaryKey(),", "  email: text('email').notNull(),", "  customerId: uuid('customer_id'),", "});"])) }),
        (ToolKind::Edit, "Edit", "src/db/schema.ts", Edit { added:Some(12), removed:Some(3), hunks:Some(vec![Hunk { line:Some(5), label:Some("users".into()), added:Some(4), removed:Some(1), lines:Some(vec![CodeLine { number:Some(3), text:"export const users = pgTable('users', {".into(), change:LineChange::Context }, CodeLine { number:Some(4), text:"  id: uuid('id').primaryKey(),".into(), change:LineChange::Context }, CodeLine { number:Some(5), text:"  email: text('email').notNull(),".into(), change:LineChange::Context }, CodeLine { number:None, text:"  customerId: uuid('customer_id'),".into(), change:LineChange::Removed }, CodeLine { number:Some(7), text:"  tenantId: uuid('tenant_id').notNull(),".into(), change:LineChange::Added }, CodeLine { number:Some(8), text:"    .references(() => tenants.id),".into(), change:LineChange::Added }, CodeLine { number:Some(9), text:"});".into(), change:LineChange::Context }]) }, Hunk {line:Some(44),label:Some("orgs".into()),added:Some(4),removed:Some(1),lines:None}, Hunk {line:Some(71),label:Some("invoices".into()),added:Some(4),removed:Some(1),lines:None}]) }),
        (ToolKind::Create, "Create", "migrations/0042_add_tenant_id.sql", Create { lines:Some(38), first_line:Some("ALTER TABLE users ADD COLUMN tenant_id uuid;".into()), content:Some(code(&["-- 0042: add tenant_id to customer tables", "BEGIN;", "ALTER TABLE users ADD COLUMN tenant_id uuid;", "ALTER TABLE orgs ADD COLUMN tenant_id uuid;", "ALTER TABLE invoices ADD COLUMN tenant_id uuid;", "CREATE INDEX users_tenant_idx ON users (tenant_id);"])) }),
        (ToolKind::Search, "Search", "“customer_id” in src/", Search { query:Some("customer_id".into()), scope:Some("src/".into()), files:Some(vec![SearchFile { path:Some("src/db/schema.ts".into()), hits:Some(5), matches:Some(code_from(8, &["customerId: uuid('customer_id'),", "customerId: uuid('customer_id').notNull(),"])) }, SearchFile { path:Some("src/api/billing.ts".into()), hits:Some(4), matches:Some(code_from(31, &["where: eq(invoices.customerId, id),", ".select({ customer_id: invoices.customerId })"])) }, SearchFile { path:Some("src/api/users.ts".into()), hits:Some(2), matches:None }, SearchFile { path:None, hits:Some(2), matches:None }, SearchFile { path:None, hits:Some(1), matches:None }]) }),
        (ToolKind::Run, "Run", "pnpm test db", Run { command:Some("pnpm test db".into()), exit_code:Some(0), duration:Some("4.2s".into()), tail:Some(output(&["✓ migrations apply cleanly", "✓ schema matches snapshot", "Tests 12 passed (12)"])), full:Some(output(&["> vitest run --dir test/db", "", "✓ test/db/migrate.test.ts (4)", "✓ test/db/schema.test.ts (8)", "", "Tests 12 passed (12)", "Duration 4.2s"])) }),
        (ToolKind::Test, "Test", "test/api", Test { target:Some("test/api".into()), passed:Some(38), failed:Some(2), failures:Some(vec![TestFailure { name:"billing › totals per tenant".into(), trace:Some(output(&["Expected: 3 invoices", "Received: 7 invoices", "at test/api/billing.test.ts:48"])) }, TestFailure { name:"users › list scoped to tenant".into(), trace:None }]) }),
        (ToolKind::Fetch, "Fetch", "postgresql.org/docs/…/sql-altertable", Fetch { url:Some("postgresql.org/docs/current/sql-altertable.html".into()), status:Some(200), title:Some("ALTER TABLE".into()), read:Some("Notes section, ~1,900 words".into()), used:Some("to check lock levels for ADD COLUMN".into()), kept:Some("2 passages quoted into the plan".into()) }),
        (ToolKind::Plan, "Plan", "tenant_id rollout", Plan { title:Some("tenant_id rollout".into()), steps:Some(vec![PlanStep { title:"Diff schema".into(), done:Some(true), owner:Some("Schema diff".into()) }, PlanStep { title:"Write migrations".into(), done:Some(true), owner:Some("Write migrations".into()) }, PlanStep { title:"Seed fixtures".into(), done:Some(true), owner:Some("Seed fixtures".into()) }, PlanStep { title:"Backfill existing rows".into(), done:Some(false), owner:Some("Backfill script".into()) }, PlanStep { title:"Verify row counts".into(), done:Some(false), owner:Some("Verify rows".into()) }]) }),
        (ToolKind::Spawn, "Spawn", "Write migrations", Spawn { child:Some("Write migrations".into()), harness:Some(Harness::ClaudeCode), model:Some(ModelIdentity { family:"Sonnet".into(), version:"5.5".into(), vendor:"ANTHROPIC".into(), hue:ModelHue::Sonnet }), effort:Some(Effort {current:3, ..effort()}), task:Some("Write migrations adding tenant_id to the six customer tables.".into()), context:Some("schema.ts, migrations/, Schema diff result".into()), reports_to:Some("API migration".into()), permissions:Some(vec![ToolPermission { capability:"read and edit files".into(), mode:PermissionMode::Allow }, ToolPermission { capability:"write to the database".into(), mode:PermissionMode::Ask }, ToolPermission { capability:"spawn agents".into(), mode:PermissionMode::Deny }, ToolPermission {capability:"run tests".into(), mode:PermissionMode::Allow}]) }),
        (ToolKind::Message, "Message", "Backfill script", Message { target:Some("Backfill script".into()), read:Some(true), exchange:Some(vec![Exchange { who:Some("API migration".into()), text:Some("Batch the backfill in 5,000-row chunks and pause between batches.".into()) }, Exchange { who:Some("Backfill script".into()), text:Some("Understood. Switching to chunked updates.".into()) }]) }),
        (ToolKind::Wait, "Wait for", "3 agents", Wait { agents:Some(vec![WaitAgent { name:"Schema diff".into(), harness:Some(Harness::Codex), status:Some(AgentStatus::Done), lane:Some(vec![LaneSegment { status:AgentStatus::Queued, fraction:0.05 }, LaneSegment { status:AgentStatus::Working, fraction:0.25 }, LaneSegment { status:AgentStatus::Done, fraction:0.7 }]) }, WaitAgent { name:"Write migrations".into(), harness:Some(Harness::ClaudeCode), status:Some(AgentStatus::Waiting), lane:Some(vec![LaneSegment { status:AgentStatus::Queued, fraction:0.1 }, LaneSegment { status:AgentStatus::Working, fraction:0.45 }, LaneSegment { status:AgentStatus::Waiting, fraction:0.45 }]) }, WaitAgent { name:"Backfill script".into(), harness:Some(Harness::Codex), status:Some(AgentStatus::Working), lane:Some(vec![LaneSegment { status:AgentStatus::Queued, fraction:0.3 }, LaneSegment { status:AgentStatus::Working, fraction:0.7 }]) }]) }),
        (ToolKind::Approval, "Approval", "Write migrations · psql", Approval { requester:Some("Write migrations".into()), command:Some("psql $STAGING_URL -f migrations/0042_add_tenant_id.sql".into()), why:Some("Applies the migration so Backfill script can start.".into()), touches:Some("staging database · 6 tables".into()), blocks:Some("Backfill script".into()) }),
        (ToolKind::Result, "Result", "from Schema diff", Result { quantities:Some("6 tables".into()), from:Some("Schema diff".into()), summary:Some("Six tables hold customer data. Two already have a tenant column under another name.".into()), full:Some("Six tables hold customer data: users, orgs, invoices, payments, sessions, audit_log. Two already have a tenant column under another name.".into()), attachments:Some(vec!["notes/schema-diff.md".into()]), used_by:Some("API migration · plan step 2".into()) }),
    ];
    calls
        .into_iter()
        .map(|(kind, verb, target, detail)| ToolCallView {
            kind,
            status: match kind {
                ToolKind::Approval => AgentStatus::Waiting,
                ToolKind::Wait | ToolKind::Plan => AgentStatus::Working,
                ToolKind::Test => AgentStatus::Failed,
                _ => AgentStatus::Done,
            },
            verb: verb.into(),
            target: Some(target.into()),
            opening: Opening::Collapsed,
            detail: Some(detail),
        })
        .collect()
}
pub fn tool_board() -> Element {
    let board = view! {
        col height:min-content gap:{px(24.0)}px fill:ground.page font-family:monospace
            font-color:ground.ink pad:{px(20.0)}px {}
    };
    for vm in specimens() {
        let row = view! {
            row height:min-content gap:{px(18.0)}px align:start {}
        };
        for (opening, width) in [
            (Opening::Collapsed, 360.0),
            (Opening::Summary, 400.0),
            (Opening::Expanded, 476.0),
        ] {
            let mut initial = vm.clone();
            initial.opening = opening;
            let value = State::new(initial);
            let change: OnOpening = Rc::new(move |opening| value.update(|v| v.opening = opening));
            let actions = ToolActions {
                approve: Some(noop()),
                always: Some(noop()),
                reject: Some(noop()),
            };
            row.adopt(&view! {
                col width:{px(width)}px height:min-content {
                    ToolCallRow data:(value) on-opening:(change) actions:(actions)
                }
            });
        }
        board.adopt(&row);
    }
    board
}
pub fn conversation() -> Element {
    let ledger = State::new(settings(false));
    let actions = LedgerActions {
        change_harness: noop(),
        change_model: noop(),
        set_effort: Rc::new(move |level| {
            ledger.update(|s| {
                s.effort.current = level;
                s.effort.at_max = s.effort.has_max && level == s.effort.levels;
            })
        }),
        toggle: Rc::new(move || ledger.update(|s| s.collapsed = !s.collapsed)),
    };
    let children = vec![
        SpawnChild {
            name: "Schema diff".into(),
            status: AgentStatus::Done,
            harness: Some(Harness::Codex),
            model: Some(model()),
            effort: Some(effort()),
        },
        SpawnChild {
            name: "Write migrations".into(),
            status: AgentStatus::Waiting,
            harness: Some(Harness::ClaudeCode),
            model: Some(ModelIdentity {
                family: "Sonnet".into(),
                version: "5.5".into(),
                vendor: "ANTHROPIC".into(),
                hue: ModelHue::Sonnet,
            }),
            effort: Some(Effort {
                current: 3,
                ..effort()
            }),
        },
        SpawnChild {
            name: "Backfill script".into(),
            status: AgentStatus::Working,
            harness: Some(Harness::Codex),
            model: Some(model()),
            effort: Some(effort()),
        },
        SpawnChild {
            name: "Seed fixtures".into(),
            status: AgentStatus::Queued,
            harness: Some(Harness::Copilot),
            model: Some(ModelIdentity::new("Sonnet", "5.5", ModelHue::Sonnet).vendor("Claude")),
            effort: Some(Effort::new(5, 1)),
        },
    ];
    let approval = ApprovalView {
        requester: Some("Write migrations asked".into()),
        path: Some("Release 2.4 / API migration / Write migrations".into()),
        heading: "Apply the migration to staging".into(),
        command: Some("psql $STAGING_URL -f migrations/0042_add_tenant_id.sql".into()),
        why: Some("Adds tenant_id to six tables so the backfill can start.".into()),
        touches: None,
        blocks: Some("Backfill script · Verify rows".into()),
    };
    let group = State::new(ActionGroupView {
        actions: vec![
            CompactAction {
                status: AgentStatus::Done,
                verb: "Read".into(),
                target: Some("src/db/schema.ts".into()),
                info: None,
            },
            CompactAction {
                status: AgentStatus::Done,
                verb: "Searched".into(),
                target: Some("“customer_id” in src/".into()),
                info: None,
            },
            CompactAction {
                status: AgentStatus::Done,
                verb: "Read".into(),
                target: Some("migrations/".into()),
                info: None,
            },
            CompactAction {
                status: AgentStatus::Done,
                verb: "Ran".into(),
                target: Some("pnpm test db".into()),
                info: Some("passed".into()),
            },
        ],
        expanded: true,
    });
    let group_toggle: OnPress = Rc::new(move || group.update(|g| g.expanded = !g.expanded));
    view! {
        col height:min-content gap:{px(28.0)}px pad:{px(24.0)}px fill:ground.page
            font-family:monospace font-color:ground.ink {
            TranscriptColumn {
                MessageRow speaker:"You" {
                    UserMessage {
                        text
                            {"Add a tenant_id column to every table that holds customer data, backfill it, and make sure nothing breaks. Split the work however you like, but ask me before anything touches the database."}
                    }
                }
                MessageRow speaker:"API migration" harness:(Some(Harness::Codex)) {
                    AgentText {
                        text
                            {"Plan: diff the schema first to find every customer table, then hand the migration, the backfill and the fixtures to separate agents so they can run in parallel. Anything that writes to the database will stop and wait for you."}
                    }
                }
                MessageRow speaker:"" {
                    ActionGroup data:(group) on-toggle:(group_toggle)
                }
                MessageRow speaker:"" {
                    SpawnBlock agents:(Live::fixed(children)) on-open-tree:(noop())
                }
                MessageRow speaker:"" {
                    ApprovalBlock data:(approval) on-approve:(noop()) on-reject:(noop())
                        on-review-diff:(noop())
                }
                MessageRow speaker:"API migration" {
                    ThinkingLine effort:5u8
                }
                Composer settings:(ledger) ledger-actions:(actions)
                    value:(State::new(String::new())) on-send:(noop()) on-attach:(noop())
                    placeholder:"Message API migration…"
            }
        }
    }
}
pub fn composer_states() -> Element {
    let board = view! {
        col height:min-content gap:{px(20.0)}px fill:ground.page font-family:monospace
            font-color:ground.ink {}
    };
    for (collapsed, opus, level, max, text) in [
        (false, false, 4, false, ""),
        (
            false,
            false,
            4,
            false,
            "Hold the backfill until I have approved the migration.",
        ),
        (true, false, 4, false, ""),
        (false, true, 3, false, ""),
        (false, true, 4, false, ""),
        (false, true, 4, true, ""),
        (true, true, 4, true, ""),
    ] {
        let mut initial = settings(collapsed);
        initial.effort.current = level;
        if opus {
            initial.harness = Harness::ClaudeCode;
            initial.model = ModelIdentity {
                family: "Opus".into(),
                version: "5.5".into(),
                vendor: "CLAUDE".into(),
                hue: ModelHue::Opus,
            };
            initial.effort.has_max = true;
            initial.effort.at_max = max;
        }
        let state = State::new(initial);
        let actions = LedgerActions {
            change_harness: noop(),
            change_model: noop(),
            set_effort: Rc::new(move |level| {
                state.update(|s| {
                    s.effort.current = level;
                    s.effort.at_max = s.effort.has_max && level == s.effort.levels;
                })
            }),
            toggle: Rc::new(move || state.update(|s| s.collapsed = !s.collapsed)),
        };
        board.adopt(&view! {
            Composer settings:(state) ledger-actions:(actions) value:(State::new(text.into()))
                on-send:(noop()) on-attach:(noop()) placeholder:"Message API migration…"
        });
    }
    board.adopt(&choice_specimen());
    board
}
fn output(lines: &[&str]) -> Vec<CodeLine> {
    lines
        .iter()
        .map(|line| CodeLine {
            number: None,
            text: (*line).into(),
            change: LineChange::Context,
        })
        .collect()
}
fn choice_specimen() -> Element {
    let open = State::new(false);
    let selected = State::new("Sol 6.1".to_owned());
    let toggle: OnPress = Rc::new(move || open.set(!open.get_untracked()));
    let choose: Rc<dyn Fn(String)> = Rc::new(move |id| selected.set(id));
    view! {
        stack width:{px(180.0)}px height:{px(44.0)}px {
            CellButton label:"Choose model" on-press:(toggle) {
                Label text:(selected)
            }
            ChoiceList open:(Some(open)) label:"Models" on-select:(choose)
                options:(Derived::new(move || vec![
                    Choice { id:"Sol 6.1".into(), label:"Sol 6.1".into(), description:Some("GPT".into()), selected:selected.get() == "Sol 6.1" },
                    Choice { id:"Opus 5.5".into(), label:"Opus 5.5".into(), description:Some("Claude".into()), selected:selected.get() == "Opus 5.5" },
                ]))
        }
    }
}
fn code_from(start: u32, lines: &[&str]) -> Vec<CodeLine> {
    let mut code = code(lines);
    for (index, line) in code.iter_mut().enumerate() {
        line.number = Some(start + index as u32);
    }
    code
}
