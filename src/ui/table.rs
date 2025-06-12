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

pub fn term_size_settings() -> SettingMinWidth {
    Settings::default()
        .with(Width::wrap(*TERM_WIDTH).priority(PriorityMax::default()))
        .with(Width::increase(*TERM_WIDTH))
    // .with(Height::limit(*TERM_HEIGHT))
    // .with(Height::increase(*TERM_HEIGHT))
}

pub fn default_style(table: &mut Table, no_headers: bool) {
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

pub struct MiseTable {
    table: comfy_table::Table,
    truncate: bool,
}

impl MiseTable {
    pub fn new(no_header: bool, headers: &[&str]) -> Self {
        let mut table = comfy_table::Table::new();
        table
            .load_preset(comfy_table::presets::NOTHING)
            .set_content_arrangement(ContentArrangement::Dynamic);
        if !console::colors_enabled() {
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

    pub fn truncate(&mut self, truncate: bool) -> &mut Self {
        self.truncate = truncate;
        self
    }

    fn header(title: impl ToString) -> Cell {
        Cell::new(title)
            .add_attribute(Attribute::Italic)
            .fg(Color::Magenta)
    }

    pub fn add_row(&mut self, row: impl Into<Row>) {
        let mut row = row.into();
        row.max_height(1);
        self.table.add_row(row);
    }

    pub fn print(&self) -> Result<()> {
        let table = self.table.to_string();
        // trim first character, skipping color characters
        let re = regex!(r"^(\x{1b}[^ ]*\d+m) ");
        for line in table.lines() {
            let line = re.replacen(line.trim(), 1, "$1");
            println!("{line}");
        }
        Ok(())
    }
}
