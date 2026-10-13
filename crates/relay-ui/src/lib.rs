//! Relay's Mosaic component library: the orchestrator design's tokens,
//! fonts and primitives. It depends only on Mosaic, so any Mosaic app can
//! use it.
//!
//! Component nodes in `view!` are bare identifiers, so callers glob-import
//! the [`prelude`]. Tokens live in [`tokens`] and are imported separately,
//! because their group names (`ground`, `status`, `model`, …) are short
//! enough to collide with application modules.
//!
//! An application installs three things once at setup:
//!
//! ```ignore
//! App::new("…")
//!     .theme(relay_ui::tokens::theme_dark())
//!     .theme(relay_ui::icons::icons())
//!     .run(|ui, _| {
//!         relay_ui::install(ui); // fonts, plus text measurement and popovers
//!         …
//!     });
//! ```

extern crate self as relay_ui;

pub mod cells;
pub mod composer;
pub mod context;
pub mod dither;
pub mod effort;
pub mod fingerprint;
pub mod fonts;
pub mod frame;
pub mod glyph;
pub mod icons;
pub mod marks;
pub mod model;
pub mod motion;
pub mod overlay;
pub mod session_header;
pub mod status;
pub mod surfaces;
pub mod text;
pub mod tokens;
pub mod tool_call;
pub mod transcript;
pub mod tree;
pub mod usage;
mod value;

#[cfg(test)]
mod d2_tests;

#[cfg(test)]
mod testing;
#[cfg(test)]
mod tests;

use mosaic::prelude::Ui;
use std::cell::RefCell;

pub use value::{Live, UiText};

/// Every component with its generated props type, plus the small value
/// types their props take.
pub mod prelude {
    pub use crate::cells::{
        Cell, CellButton, CellButtonProps, CellProps, Chip, ChipProps, Hairline, HairlineProps,
        IconCell, IconCellProps, Kbd, KbdProps, LinkButton, LinkButtonProps, OnPress,
        PrimaryButton, PrimaryButtonProps, Rule, RuledCol, RuledColProps, RuledRow, RuledRowProps,
        SecondaryButton, SecondaryButtonProps,
    };
    pub use crate::composer::*;
    pub use crate::context::{
        ContextGauge, ContextGaugeProps, ContextPanel, ContextPanelProps, ContextRing,
        ContextRingProps, ContextShare, ContextView,
    };
    pub use crate::dither::{Dither, DitherProps, Gradient, Wash, WashProps, live_reach};
    pub use crate::effort::{EffortTrack, EffortTrackProps, OnEffort, Orientation, TrackSize};
    pub use crate::fingerprint::{FingerprintStrip, FingerprintStripProps, ToolCount};
    pub use crate::frame::{Corner, Cut, CutCorner, CutCornerProps};
    pub use crate::glyph::{Harness, HarnessGlyph, HarnessGlyphProps};
    pub use crate::marks::{Mark, PathText, PathTextProps};
    pub use crate::model::{Effort, ModelHue, ModelIdentity};
    pub use crate::motion::{Breath, breathe};
    pub use crate::overlay::{Popover, PopoverContent, PopoverProps, Tooltip, TooltipProps};
    pub use crate::session_header::{
        ControlBlock, ControlBlockProps, SessionHeader, SessionHeaderProps,
    };
    pub use crate::status::{AgentStatus, StatusSquare, StatusSquareProps};
    pub use crate::surfaces::{
        BoardColumn, BoardColumnProps, CommandPalette, CommandPaletteProps, ConnectionStatus,
        ConnectionStatusProps, Field, FieldProps, HarnessCard, HarnessCardProps, HeaderCell,
        HeaderCellProps, IssueCard, IssueCardProps, IssueInspector, IssueInspectorProps, Module,
        ModuleProps, Notice, NoticeProps, PageHeader, PageHeaderProps, PermissionMatrix,
        PermissionMatrixProps, Segments, SegmentsProps, Select, SelectProps, Stepper, StepperProps,
        Toggle, ToggleProps,
    };
    pub use crate::text::{
        Ellipsis, EllipsisProps, Label, LabelProps, SerifSize, SerifTitle, SerifTitleProps,
        TextRole, Tone, VerticalLabel, VerticalLabelProps, Weight,
    };
    pub use crate::tool_call::*;
    pub use crate::transcript::*;
    pub use crate::tree::{
        AgentNode, AgentSidebar, AgentSidebarProps, AgentTreeRow, AgentTreeRowProps,
        ChildStatusBar, ChildStatusBarProps, Elbows, ElbowsProps, FocusCrumb, FocusCrumbProps,
        FoldedBranch, FoldedBranchProps, NeedsYou, NeedsYouCard, NeedsYouCardProps, NeedsYouProps,
        StatusCounts, StatusCountsProps,
    };
    pub use crate::usage::{
        ForecastBar, ForecastBarProps, UsageBar, UsageBarProps, UsageDetail, UsageDetailProps,
        UsageFoot, UsageFootProps, UsageRow, UsageRowProps,
    };
    pub use crate::{Live, UiText};
}

thread_local! {
    static ATTACHED: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

/// Installs the bundled fonts into `ui` and attaches it, so components can
/// measure text ([`text::Ellipsis`], [`text::VerticalLabel`]) and watch for
/// outside clicks ([`overlay::Popover`]).
pub fn install(ui: &Ui) {
    fonts::install(&mut ui.fonts().borrow_mut());
    attach(ui);
}

/// Attaches `ui` without changing its fonts. One `Ui` is attached per
/// thread; attaching again replaces it.
pub fn attach(ui: &Ui) {
    ATTACHED.with(|slot| *slot.borrow_mut() = Some(ui.clone()));
}

/// The attached `Ui`, if any.
pub(crate) fn attached() -> Option<Ui> {
    ATTACHED.with(|slot| slot.borrow().clone())
}
