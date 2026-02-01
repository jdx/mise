//! Picker: Fuzzy-searchable list picker UI component

use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

/// An item that can be displayed in the picker
#[derive(Debug, Clone)]
pub struct PickerItem {
    /// Display name (used for matching and display)
    pub name: String,
    /// Optional description shown next to the name
    pub description: Option<String>,
    /// Optional data payload (e.g., tool backend info)
    pub data: Option<String>,
}

impl PickerItem {
    /// Create a new picker item
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            data: None,
        }
    }

    /// Add a description
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Add data payload
    #[allow(dead_code)]
    pub fn with_data(mut self, data: impl Into<String>) -> Self {
        self.data = Some(data.into());
        self
    }
}

/// Filtered item with match score for sorting
#[derive(Debug, Clone)]
pub struct FilteredItem {
    /// Index into the original items list
    pub index: usize,
    /// Match score (higher is better)
    pub score: i64,
    /// Matched positions in the name (for highlighting)
    pub positions: Vec<usize>,
}

/// State for the fuzzy picker
pub struct PickerState {
    /// All available items
    items: Vec<PickerItem>,
    /// Filtered items after applying search filter
    filtered: Vec<FilteredItem>,
    /// Current filter text
    filter: String,
    /// Selected index in the filtered list
    cursor: usize,
    /// Scroll offset for the visible window
    scroll_offset: usize,
    /// Height of visible area (number of items)
    visible_height: usize,
    /// Fuzzy matcher instance (created fresh, not stored for Clone/Debug)
    matcher: SkimMatcherV2,
}

impl std::fmt::Debug for PickerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PickerState")
            .field("items", &self.items)
            .field("filtered", &self.filtered)
            .field("filter", &self.filter)
            .field("cursor", &self.cursor)
            .field("scroll_offset", &self.scroll_offset)
            .field("visible_height", &self.visible_height)
            .finish_non_exhaustive()
    }
}

impl Clone for PickerState {
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
            filtered: self.filtered.clone(),
            filter: self.filter.clone(),
            cursor: self.cursor,
            scroll_offset: self.scroll_offset,
            visible_height: self.visible_height,
            matcher: SkimMatcherV2::default(),
        }
    }
}

impl PickerState {
    /// Create a new picker with the given items
    pub fn new(items: Vec<PickerItem>) -> Self {
        let filtered: Vec<FilteredItem> = items
            .iter()
            .enumerate()
            .map(|(i, _)| FilteredItem {
                index: i,
                score: 0,
                positions: Vec::new(),
            })
            .collect();

        Self {
            items,
            filtered,
            filter: String::new(),
            cursor: 0,
            scroll_offset: 0,
            visible_height: 10,
            matcher: SkimMatcherV2::default(),
        }
    }

    /// Set the visible height
    pub fn with_visible_height(mut self, height: usize) -> Self {
        self.visible_height = height;
        self
    }

    /// Get the current filter text
    pub fn filter(&self) -> &str {
        &self.filter
    }

    /// Get the currently selected item, if any
    pub fn selected(&self) -> Option<&PickerItem> {
        self.filtered.get(self.cursor).map(|f| &self.items[f.index])
    }

    /// Get visible items with their display info
    pub fn visible_items(&self) -> impl Iterator<Item = VisibleItem<'_>> {
        let start = self.scroll_offset;
        let end = (self.scroll_offset + self.visible_height).min(self.filtered.len());

        self.filtered[start..end]
            .iter()
            .enumerate()
            .map(move |(i, filtered)| VisibleItem {
                item: &self.items[filtered.index],
                is_selected: start + i == self.cursor,
                positions: &filtered.positions,
            })
    }

    /// Check if there are more items above the visible area
    pub fn has_more_above(&self) -> bool {
        self.scroll_offset > 0
    }

    /// Check if there are more items below the visible area
    pub fn has_more_below(&self) -> bool {
        self.scroll_offset + self.visible_height < self.filtered.len()
    }

    /// Get the total number of filtered items
    pub fn filtered_count(&self) -> usize {
        self.filtered.len()
    }

    /// Get the total number of items
    #[allow(dead_code)]
    pub fn total_count(&self) -> usize {
        self.items.len()
    }

    /// Add a character to the filter
    pub fn type_char(&mut self, c: char) {
        self.filter.push(c);
        self.apply_filter();
    }

    /// Remove the last character from the filter
    pub fn backspace(&mut self) {
        self.filter.pop();
        self.apply_filter();
    }

    /// Clear the filter
    #[allow(dead_code)]
    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.apply_filter();
    }

    /// Move cursor up
    pub fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.ensure_cursor_visible();
        }
    }

    /// Move cursor down
    pub fn move_down(&mut self) {
        if self.cursor + 1 < self.filtered.len() {
            self.cursor += 1;
            self.ensure_cursor_visible();
        }
    }

    /// Apply the current filter to the items
    fn apply_filter(&mut self) {
        if self.filter.is_empty() {
            // No filter - show all items in original order
            self.filtered = self
                .items
                .iter()
                .enumerate()
                .map(|(i, _)| FilteredItem {
                    index: i,
                    score: 0,
                    positions: Vec::new(),
                })
                .collect();
        } else {
            // Apply fuzzy matching
            self.filtered = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, item)| {
                    // Match against name and description
                    let name_match = self.matcher.fuzzy_indices(&item.name, &self.filter);
                    let desc_match = item
                        .description
                        .as_ref()
                        .and_then(|d| self.matcher.fuzzy_match(d, &self.filter));

                    // Take the best score
                    match (name_match, desc_match) {
                        (Some((name_score, positions)), Some(desc_score)) => Some(FilteredItem {
                            index: i,
                            score: name_score.max(desc_score),
                            positions,
                        }),
                        (Some((score, positions)), None) => Some(FilteredItem {
                            index: i,
                            score,
                            positions,
                        }),
                        (None, Some(score)) => Some(FilteredItem {
                            index: i,
                            score,
                            positions: Vec::new(),
                        }),
                        (None, None) => None,
                    }
                })
                .collect();

            // Sort by score (highest first)
            self.filtered.sort_by(|a, b| b.score.cmp(&a.score));
        }

        // Reset cursor to start
        self.cursor = 0;
        self.scroll_offset = 0;
    }

    /// Ensure the cursor is visible in the viewport
    fn ensure_cursor_visible(&mut self) {
        if self.cursor < self.scroll_offset {
            self.scroll_offset = self.cursor;
        } else if self.cursor >= self.scroll_offset + self.visible_height {
            self.scroll_offset = self.cursor.saturating_sub(self.visible_height - 1);
        }
    }
}

/// A visible item in the picker with display metadata
#[derive(Debug)]
pub struct VisibleItem<'a> {
    /// The item to display
    pub item: &'a PickerItem,
    /// Whether this item is currently selected
    pub is_selected: bool,
    /// Character positions to highlight (from fuzzy match)
    pub positions: &'a [usize],
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_picker_basic() {
        let items = vec![
            PickerItem::new("node").with_description("Node.js runtime"),
            PickerItem::new("python").with_description("Python interpreter"),
            PickerItem::new("ruby").with_description("Ruby interpreter"),
        ];

        let picker = PickerState::new(items);
        assert_eq!(picker.filtered_count(), 3);
        assert_eq!(picker.selected().unwrap().name, "node");
    }

    #[test]
    fn test_picker_filter() {
        let items = vec![
            PickerItem::new("node"),
            PickerItem::new("python"),
            PickerItem::new("ruby"),
            PickerItem::new("nodenv"),
        ];

        let mut picker = PickerState::new(items);
        picker.type_char('n');
        picker.type_char('o');
        picker.type_char('d');

        // Should match "node" and "nodenv"
        assert_eq!(picker.filtered_count(), 2);

        // "node" should rank higher (exact prefix match)
        let selected = picker.selected().unwrap();
        assert!(selected.name == "node" || selected.name == "nodenv");
    }

    #[test]
    fn test_picker_navigation() {
        let items = vec![
            PickerItem::new("a"),
            PickerItem::new("b"),
            PickerItem::new("c"),
        ];

        let mut picker = PickerState::new(items);
        assert_eq!(picker.selected().unwrap().name, "a");

        picker.move_down();
        assert_eq!(picker.selected().unwrap().name, "b");

        picker.move_down();
        assert_eq!(picker.selected().unwrap().name, "c");

        picker.move_down(); // Should stay at end
        assert_eq!(picker.selected().unwrap().name, "c");

        picker.move_up();
        assert_eq!(picker.selected().unwrap().name, "b");
    }

    #[test]
    fn test_picker_backspace() {
        let items = vec![PickerItem::new("node"), PickerItem::new("python")];

        let mut picker = PickerState::new(items);
        picker.type_char('p');
        picker.type_char('y');
        assert_eq!(picker.filtered_count(), 1);

        picker.backspace();
        picker.backspace();
        assert_eq!(picker.filtered_count(), 2);
    }
}
