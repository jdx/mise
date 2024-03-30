use console::style;
use tabled::settings::object::{Columns, Rows};
use tabled::settings::peaker::PriorityMax;
use tabled::settings::width::{MinWidth, Wrap};
use tabled::settings::{Disable, Format, Margin, Modify, Padding, Settings, Style, Width};
use tabled::Table;

use crate::env::TERM_WIDTH;

type SettingPriority = Settings<Settings, Wrap<usize, PriorityMax>>;
type SettingMinWidth = Settings<SettingPriority, MinWidth>;
// type SettingCellHeightLimit = Settings<SettingMinWidth, CellHeightLimit>;
// type SettingCellHeightIncrease = Settings<SettingCellHeightLimit, CellHeightIncrease>;

pub fn term_size_settings() -> SettingMinWidth {
    Settings::default()
        .with(Width::wrap(*TERM_WIDTH).priority::<PriorityMax>())
        .with(Width::increase(*TERM_WIDTH))
    // .with(Height::limit(*TERM_HEIGHT))
    // .with(Height::increase(*TERM_HEIGHT))
}

pub fn default_style(table: &mut Table, no_headers: bool) {
    let header = |h: &_| style(h).italic().magenta().to_string();

    if no_headers || !console::user_attended() || cfg!(test) {
        table.with(Disable::row(Rows::first()));
    } else {
        table.with(Modify::new(Rows::first()).with(Format::content(header)));
    }
    table.with(Style::empty());
    if console::user_attended() || cfg!(test) {
        table.with(term_size_settings());
    }
    table
        .with(Margin::new(0, 0, 0, 0))
        .with(Modify::new(Columns::first()).with(Padding::new(0, 1, 0, 0)))
        .with(Modify::new(Columns::last()).with(Padding::zero()));
}

pub fn disable_columns(table: &mut Table, col_idxs: Vec<usize>) {
    for idx in col_idxs {
        table.with(Disable::column(Columns::single(idx)));
    }
}
