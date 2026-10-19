use egui::{Frame, Margin, Response, Ui};

use crate::design::Combine;
use crate::edit::Edit;
use crate::ui::commands::{Cx, Entry};
use crate::ui::icons::Icon;
use crate::ui::popover::{Tile, popover, tile_grid};

const TOP: [Edit; 3] = [Edit::Combine(Combine::Slice), Edit::Combine(Combine::Weld), Edit::Flatten];
const BOTTOM: [Edit; 4] = [Edit::Combine(Combine::Unite), Edit::Combine(Combine::Subtract), Edit::Combine(Combine::Intersect), Edit::Combine(Combine::Exclude)];

/// The popover's tiles: Slice, Weld and Flatten (Unflatten for a flattened image) above, the four
/// booleans below.
pub(in crate::ui) fn entries(cx: &Cx) -> [Vec<Entry>; 2] {
    let mut top: Vec<Entry> = TOP[..2].iter().map(|e| cx.entry(e.clone())).collect();
    top.push(cx.either(Edit::Flatten, Edit::Unflatten));
    [top, BOTTOM.iter().map(|e| cx.entry(e.clone())).collect()]
}

/// Whether the Combine button has anything to offer.
pub fn available(cx: &Cx) -> bool {
    entries(cx).iter().flatten().any(Entry::enabled)
}

pub fn menu(ui: &Ui, anchor: &Response, cx: &Cx) -> Option<Edit> {
    let [top, bottom] = entries(cx);
    // The reference pads the grid well inside the menu frame and opens up the gap between rows.
    let pad = Margin::symmetric(28, 18);
    popover(ui, anchor, "menu", 4.0 * 72.0 + 3.0 * 12.0 + pad.sum().x, |ui| {
        Frame::NONE
            .inner_margin(pad)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 28.0;
                let top = grid(ui, &top);
                let bottom = grid(ui, &bottom);
                top.or(bottom)
            })
            .inner
    })
}

fn grid(ui: &mut Ui, entries: &[Entry]) -> Option<Edit> {
    let tips: Vec<String> = entries.iter().map(Entry::tip_body).collect();
    let tiles: Vec<Tile> =
        entries.iter().zip(&tips).map(|(e, tip)| Tile { icon: e.info.icon.unwrap_or(Icon::Combine), label: e.label, enabled: e.enabled(), tip }).collect();
    tile_grid(ui, 4, &tiles).map(|i| entries[i].edit().clone())
}
