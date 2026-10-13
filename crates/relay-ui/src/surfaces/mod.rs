//! Surfaces the design pack does not draw, built in its language from the
//! primitives: connection status, page chrome, notices, the command
//! palette, form controls, board columns and cards, the issue inspector,
//! the permission matrix and harness cards. They take plain strings and
//! callbacks.

mod board;
mod connection;
mod form;
mod page;
#[cfg(test)]
mod tests;

pub use board::{
    BoardColumn, BoardColumnProps, FlagColumn, HarnessAuth, HarnessCard, HarnessCardProps,
    IssueCard, IssueCardProps, IssueInspector, IssueInspectorProps, IssueView, OnFlag,
    OnPermission, Permission, PermissionMatrix, PermissionMatrixProps, PermissionRow,
};
pub use connection::{
    Connection, ConnectionInfo, ConnectionStatus, ConnectionStatusProps, latency_bars,
};
pub use form::{
    Field, FieldProps, OnIndex, OnNumber, OnSwitch, Segments, SegmentsProps, Select, SelectProps,
    Stepper, StepperProps, TextInput, TextInputProps, Toggle, ToggleProps,
};
pub use page::{
    Command, CommandPalette, CommandPaletteProps, HeaderCell, HeaderCellProps, Module, ModuleProps,
    Notice, NoticeAction, NoticeProps, NoticeTone, OnCommand, PageHeader, PageHeaderProps,
    ReadoutCell, ReadoutCellProps, filter_commands,
};
