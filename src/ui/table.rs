use crate::Result;
use crate::env::TERM_WIDTH;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Row};
use console::style;
use itertools::Itertools;
use tabled::Table;
use tabled::settings::object::{Columns, Rows};
use tabled::settings::peaker::PriorityMax;
use tabled::settings::width::{MinWidth, Wrap};
use tabled::settings::{Format, Margin, Modify, Padding, Remove, Settings, Style, Width};
use xx::regex;

type SettingPriority = Settings<Settings, Wrap<usize, PriorityMax>>;
type SettingMinWidth = Settings<SettingPriority, MinWidth>;
// type SettingCellHeightLimit = Settings<SettingMinWidth, CellHeightLimit>;
// type SettingCellHeightIncrease = Settings<SettingCellHeightLimit, CellHeightIncrease>;

pub(crate) fn term_size_settings() -> SettingMinWidth {
    Settings::default()
        .with(Width::wrap(*TERM_WIDTH).priority(PriorityMax::default()))
        .with(Width::increase(*TERM_WIDTH))
    // .with(Height::limit(*TERM_HEIGHT))
    // .with(Height::increase(*TERM_HEIGHT))
}

/// Style `table` and print it, with the fill taken off the end of each row.
///
/// `tabled` brings every cell up to its column's width, so the last column leaves trailing spaces
/// on every row shorter than the widest one. The `Padding::zero()` below does not prevent that:
/// padding is the space *around* a cell, and the fill that brings it up to the column width is
/// added separately. [`MiseTable::print`] already trims the comfy_table side the same way.
///
/// The only entry point on purpose. `default_style` stays private so a table cannot be printed
/// without this step, which is how the trailing spaces got there in the first place.
pub(crate) fn print(table: &mut Table, no_headers: bool) -> Result<()> {
    default_style(table, no_headers);
    // One `miseprintln!`, not one per line: this way the newlines land exactly where
    // `miseprintln!("{table}")` used to put them, including for a table with no rows, and the only
    // difference is the trailing space.
    let rendered = table.to_string();
    let trimmed = rendered
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    miseprintln!("{trimmed}");
    Ok(())
}

fn default_style(table: &mut Table, no_headers: bool) {
    let header = |h: &_| style(h).italic().magenta().to_string();

    if no_headers || !console::user_attended() || cfg!(test) {
        table.with(Remove::row(Rows::first()));
    } else {
        table.with(Modify::new(Rows::first()).with(Format::content(header)));
    }
    table.with(Style::empty());
    if console::user_attended() && !cfg!(test) {
        table.with(term_size_settings());
    }
    table
        .with(Margin::new(0, 0, 0, 0))
        .with(Modify::new(Columns::first()).with(Padding::new(0, 1, 0, 0)))
        .with(Modify::new(Columns::last()).with(Padding::zero()));
}

pub(crate) struct MiseTable {
    table: comfy_table::Table,
    truncate: bool,
}

impl MiseTable {
    pub(crate) fn new(no_header: bool, headers: &[&str]) -> Self {
        let mut table = comfy_table::Table::new();
        table
            .load_style(comfy_table::presets::NOTHING)
            .set_truncation_indicator("...")
            .set_content_arrangement(ContentArrangement::Dynamic);
        // Pin the width when the user overrides it (e.g. MISE_TERM_WIDTH in CI).
        // comfy_table does its own terminal detection, which fails in non-ttys,
        // so this is what makes the override actually affect `mise ls` et al.
        if let Some(w) = *crate::env::TERM_WIDTH_OVERRIDE {
            table.set_width(w.min(u16::MAX as usize) as u16);
        }
        if console::colors_enabled() {
            table.enforce_styling();
        } else {
            table.force_no_tty();
        }
        if !no_header && console::user_attended() {
            let headers = headers.iter().map(Self::header).collect_vec();
            table.set_header(headers);
        }
        Self {
            table,
            truncate: false,
        }
    }

    pub(crate) fn truncate(&mut self, truncate: bool) -> &mut Self {
        self.truncate = truncate;
        self
    }

    fn header(title: impl ToString) -> Cell {
        Cell::new(title)
            .add_attribute(Attribute::Italic)
            .fg(Color::Magenta)
    }

    pub(crate) fn add_row(&mut self, row: impl Into<Row>) {
        let mut row = row.into();
        row.max_height(1);
        self.table.add_row(row);
    }

    pub(crate) fn print(&self) -> Result<()> {
        let table = self.table.to_string();
        // trim first character, skipping color characters
        let re = regex!(r"^(\x{1b}[^ ]*\d+m) ");
        for line in table.lines() {
            let line = line.strip_prefix(' ').unwrap_or(line);
            let line = re.replacen(line, 1, "$1");
            let line = line.trim_end();
            calm_io::stdoutln!("{line}")?;
        }
        Ok(())
    }
}
