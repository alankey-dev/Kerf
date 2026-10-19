//! Draws a system family's name in its own face, as the font menu does. A scanned catalogue's faces
//! load on a background thread when their row first shows, one repaint per drained batch, and join
//! egui's fonts in one batch per frame; until then the row uses the UI font, so scrolling the menu
//! never waits on disk. An in-memory catalogue (the tests') loads them inline.

use std::collections::HashMap;
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex, PoisonError};

use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{Context, FontData, FontFamily, Id};

use crate::system_fonts::{FontCatalogue, PreviewFace, Previews as Loading};

#[derive(Clone, Default)]
struct Previews(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    status: HashMap<String, Status>,
    /// Faces read and not yet handed to egui.
    loaded: Vec<(String, Option<PreviewFace>)>,
    loader: Option<Sender<String>>,
}

enum Status {
    Loading,
    /// Handed to egui in this pass; usable from the next.
    Added(u64),
    Unavailable,
}

/// `family`'s own face once it has loaded, else `None` (draw in the UI font meanwhile).
pub fn family(ctx: &Context, family: &str) -> Option<FontFamily> {
    let previews = ctx.data_mut(|d| d.get_temp_mut_or_default::<Previews>(Id::new("font previews")).clone());
    let mut state = previews.0.lock().unwrap_or_else(PoisonError::into_inner);
    let pass = ctx.cumulative_pass_nr();
    let arrived = std::mem::take(&mut state.loaded);
    if !arrived.is_empty() {
        // One more pass so the rows switch to the faces added in this one.
        ctx.request_repaint();
    }
    for (name, face) in arrived {
        let status = match face {
            Some(PreviewFace { bytes, index }) => {
                let data = FontData { font: bytes.into(), index, tweak: Default::default() };
                ctx.add_font(FontInsert::new(&key(&name), data, vec![InsertFontFamily { family: FontFamily::Name(key(&name).into()), priority: FontPriority::Highest }]));
                Status::Added(pass)
            }
            None => Status::Unavailable,
        };
        state.status.insert(name, status);
    }
    match state.status.get(family) {
        Some(Status::Added(added)) if *added < pass => Some(FontFamily::Name(key(family).into())),
        Some(_) => None,
        None => {
            state.status.insert(family.to_owned(), Status::Loading);
            let fonts = crate::system_fonts::current();
            match fonts.previews() {
                Loading::Inline => {
                    let face = fonts.preview(family);
                    state.loaded.push((family.to_owned(), face));
                    ctx.request_repaint();
                }
                Loading::Background => {
                    let loader = state.loader.get_or_insert_with(|| spawn_loader(ctx.clone(), previews.clone(), fonts));
                    _ = loader.send(family.to_owned());
                }
            }
            None
        }
    }
}

fn key(family: &str) -> String {
    format!("preview:{family}")
}

fn spawn_loader(ctx: Context, previews: Previews, fonts: FontCatalogue) -> Sender<String> {
    let (tx, rx) = channel::<String>();
    std::thread::Builder::new()
        .name("font previews".into())
        .spawn(move || {
            while let Ok(first) = rx.recv() {
                let batch: Vec<_> = std::iter::once(first)
                    .chain(rx.try_iter())
                    .map(|name| {
                        let face = fonts.preview(&name);
                        (name, face)
                    })
                    .collect();
                previews.0.lock().unwrap_or_else(PoisonError::into_inner).loaded.extend(batch);
                ctx.request_repaint();
            }
        })
        .expect("spawn the font preview thread");
    tx
}
