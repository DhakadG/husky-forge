#![cfg_attr(windows, windows_subsystem = "windows")]
//! Husky Forge desktop app: Slint front end over forge-core.
mod presets;
mod platform;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use anyhow::{Context, Result};
use forge_core::{Control, Event, Format, History, Impact, MetaMode, Mode, Options, Plan, Resize, impact::human, plan, run_with};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

slint::include_modules!();

#[derive(Default)]
struct State {
    paths: Vec<PathBuf>,
    plan: Option<Arc<Plan>>,
    ctl: Arc<Control>,
}

fn main() -> Result<()> {
    let ui = App::new()?;
    platform::decorate(&ui);
    let st = Rc::new(RefCell::new(State::default()));
    ui.set_files(ModelRc::new(VecModel::<FileRow>::default()));
    ui.set_impact_lines(ModelRc::new(VecModel::<SharedString>::default()));
    ui.set_presets(ModelRc::new(VecModel::from(presets::list())));

    let add = |ui: &App, st: &Rc<RefCell<State>>, new: Vec<PathBuf>| {
        let mut s = st.borrow_mut();
        for p in new {
            if !s.paths.contains(&p) {
                s.paths.push(p);
            }
        }
        drop(s);
        replan(ui, st);
    };

    ui.on_add_files({
        let (ui, st) = (ui.as_weak(), st.clone());
        move || {
            if let Some(ps) = rfd::FileDialog::new().set_title("Add photos").pick_files() {
                add(&ui.unwrap(), &st, ps);
            }
        }
    });
    ui.on_add_folder({
        let (ui, st) = (ui.as_weak(), st.clone());
        move || {
            if let Some(ps) = rfd::FileDialog::new().set_title("Add folder").pick_folders() {
                add(&ui.unwrap(), &st, ps);
            }
        }
    });
    ui.on_accepts(|data| data.has_file_paths());
    ui.on_dropped({
        let (ui, st) = (ui.as_weak(), st.clone());
        move |data| {
            if let Ok(paths) = data.file_paths() {
                add(&ui.unwrap(), &st, paths.map(PathBuf::from).collect());
            }
        }
    });
    ui.on_clear({
        let (ui, st) = (ui.as_weak(), st.clone());
        move || {
            st.borrow_mut().paths.clear();
            replan(&ui.unwrap(), &st);
        }
    });
    ui.on_pick_lut({
        let ui = ui.as_weak();
        move || {
            if let Some(p) = rfd::FileDialog::new().add_filter("LUT", &["cube"]).pick_file() {
                ui.unwrap().set_lut_path(p.to_string_lossy().to_string().into());
            }
        }
    });
    ui.on_options_changed({
        let (ui, st) = (ui.as_weak(), st.clone());
        move || replan(&ui.unwrap(), &st)
    });
    ui.on_save_preset({
        let ui = ui.as_weak();
        move || {
            let ui = ui.unwrap();
            let name = ui.get_preset_name().to_string();
            match options_from(&ui).and_then(|o| presets::save(&name, &o)) {
                Ok(()) => ui.set_presets(ModelRc::new(VecModel::from(presets::list()))),
                Err(e) => ui.set_status_line(format!("preset: {e:#}").into()),
            }
        }
    });
    ui.on_load_preset({
        let (ui, st) = (ui.as_weak(), st.clone());
        move |name| {
            let ui = ui.unwrap();
            match presets::load(&name) {
                Ok(o) => {
                    apply_options(&ui, &o);
                    replan(&ui, &st);
                }
                Err(e) => ui.set_status_line(format!("preset: {e:#}").into()),
            }
        }
    });
    ui.on_undo_last({
        let ui = ui.as_weak();
        move || {
            let ui = ui.unwrap();
            let msg = (|| -> Result<String> {
                let mut h = History::open(&History::default_path())?;
                let job = h.jobs(1)?.into_iter().next().context("no jobs yet")?;
                let n = h.undo(job.id)?;
                Ok(format!("job #{} undone, {n} files restored", job.id))
            })();
            ui.set_status_line(msg.unwrap_or_else(|e| format!("undo: {e:#}")).into());
        }
    });
    ui.on_cancel({
        let st = st.clone();
        move || st.borrow().ctl.cancel.store(true, Ordering::Relaxed)
    });
    ui.on_pause({
        let (ui, st) = (ui.as_weak(), st.clone());
        move || {
            let ctl = st.borrow().ctl.clone();
            let now = !ctl.pause.load(Ordering::Relaxed);
            ctl.pause.store(now, Ordering::Relaxed);
            ui.unwrap().set_paused(now);
        }
    });
    ui.on_start({
        let (ui, st) = (ui.as_weak(), st.clone());
        move || start(&ui.unwrap(), &st)
    });

    // Paths on the command line: Explorer / Finder "Forge with Husky" integration and plain scripting.
    let argv: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).filter(|p| p.exists()).collect();
    if !argv.is_empty() {
        add(&ui, &st, argv);
    }

    ui.run()?;
    Ok(())
}

fn options_from(ui: &App) -> Result<Options> {
    let num = |s: SharedString, what: &str| -> Result<f64> { s.trim().parse::<f64>().with_context(|| format!("{what}: not a number")) };
    let wh = || -> Result<(u32, u32)> { Ok((num(ui.get_resize_w(), "width")? as u32, num(ui.get_resize_h(), "height")? as u32)) };
    let resize = match ui.get_resize_index() {
        0 => Resize::None,
        1 => Resize::Cap { mp: num(ui.get_max_mp(), "max MP")? },
        2 => wh().map(|(w, h)| Resize::Fit { w, h })?,
        3 => wh().map(|(w, h)| Resize::Fill { w, h })?,
        _ => wh().map(|(w, h)| Resize::Pad { w, h, rgb: [0, 0, 0] })?,
    };
    let lut = ui.get_lut_path().trim().to_string();
    Ok(Options {
        format: [None, Some(Format::Jpeg), Some(Format::Png), Some(Format::Webp), Some(Format::Avif), Some(Format::Jxl)][ui.get_format_index().clamp(0, 5) as usize],
        quality: ui.get_quality().round() as u8,
        resize,
        lut: (!lut.is_empty()).then(|| PathBuf::from(lut)),
        target_bytes: (num(ui.get_target_mb(), "target size")? * 1024.0 * 1024.0) as u64,
        meta: [MetaMode::Keep, MetaMode::StripGps, MetaMode::Strip][ui.get_meta_index().clamp(0, 2) as usize],
        mode: [Mode::Copy, Mode::Archive, Mode::Replace][ui.get_originals_index().clamp(0, 2) as usize],
        recursive: true,
        min_bytes: if ui.get_include_small() { 0 } else { Options::default().min_bytes },
        skip_sidecar: ui.get_skip_sidecar(),
        only_if_smaller: ui.get_only_if_smaller(),
        workers: num(ui.get_workers(), "workers")? as usize,
    })
}

fn apply_options(ui: &App, o: &Options) {
    ui.set_format_index(match o.format {
        None => 0,
        Some(Format::Jpeg) => 1,
        Some(Format::Png) => 2,
        Some(Format::Webp) => 3,
        Some(Format::Avif) => 4,
        Some(Format::Jxl) => 5,
    });
    ui.set_quality(o.quality as f32);
    match o.resize {
        Resize::None => ui.set_resize_index(0),
        Resize::Cap { mp } => {
            ui.set_resize_index(1);
            ui.set_max_mp(format!("{mp}").into());
        }
        Resize::Fit { w, h } | Resize::Fill { w, h } | Resize::Pad { w, h, .. } => {
            ui.set_resize_index(match o.resize {
                Resize::Fit { .. } => 2,
                Resize::Fill { .. } => 3,
                _ => 4,
            });
            ui.set_resize_w(w.to_string().into());
            ui.set_resize_h(h.to_string().into());
        }
    }
    ui.set_lut_path(o.lut.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default().into());
    ui.set_target_mb(format!("{:.2}", o.target_bytes as f64 / 1048576.0).trim_end_matches('0').trim_end_matches('.').to_string().into());
    ui.set_meta_index(match o.meta {
        MetaMode::Keep => 0,
        MetaMode::StripGps => 1,
        MetaMode::Strip => 2,
    });
    ui.set_originals_index(match o.mode {
        Mode::Copy => 0,
        Mode::Archive => 1,
        Mode::Replace => 2,
    });
    ui.set_include_small(o.min_bytes == 0);
    ui.set_skip_sidecar(o.skip_sidecar);
    ui.set_only_if_smaller(o.only_if_smaller);
    ui.set_workers(o.workers.to_string().into());
}

/// Re-walk the sources with the current options; show the estimate and the planned file list.
fn replan(ui: &App, st: &Rc<RefCell<State>>) {
    let o = match options_from(ui) {
        Ok(o) => o,
        Err(e) => {
            ui.set_status_line(format!("{e:#}").into());
            return;
        }
    };
    let mut s = st.borrow_mut();
    let p = plan(&s.paths, &o);
    ui.set_file_count(p.items.len() as i32);
    ui.set_total_size(human(p.total_bytes()).into());
    ui.set_status_line(if p.skipped.is_empty() { "".into() } else { format!("{} skipped ({})", p.skipped.len(), p.skipped[0].1).into() });
    let lines: Vec<SharedString> = if p.items.is_empty() { vec![] } else { Impact::estimate(&p, &o).lines().into_iter().map(Into::into).collect() };
    ui.get_impact_lines().as_any().downcast_ref::<VecModel<SharedString>>().unwrap().set_vec(lines);
    let rows: Vec<FileRow> = p
        .items
        .iter()
        .map(|i| FileRow { name: name_of(&i.path), before: human(i.size).into(), after: "".into(), quality: "".into(), status: "planned".into(), failed: false })
        .chain(p.skipped.iter().map(|(path, why)| FileRow { name: name_of(path), before: "".into(), after: "".into(), quality: "".into(), status: format!("skip: {why}").into(), failed: false }))
        .collect();
    ui.get_files().as_any().downcast_ref::<VecModel<FileRow>>().unwrap().set_vec(rows);
    ui.set_done(0);
    ui.set_total(p.items.len() as i32);
    s.plan = Some(Arc::new(p));
}

fn name_of(p: &std::path::Path) -> SharedString {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default().into()
}

fn start(ui: &App, st: &Rc<RefCell<State>>) {
    let o = match options_from(ui) {
        Ok(o) => o,
        Err(e) => return ui.set_status_line(format!("{e:#}").into()),
    };
    let plan = match st.borrow().plan.clone() {
        Some(p) if !p.items.is_empty() => p,
        _ => return,
    };
    let ctl = Arc::new(Control::default());
    st.borrow_mut().ctl = ctl.clone();
    ui.set_paused(false);
    let index: HashMap<PathBuf, usize> = plan.items.iter().enumerate().map(|(i, it)| (it.path.clone(), i)).collect();
    ui.set_running(true);
    ui.set_done(0);
    ui.set_status_line("".into());
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let w = weak.clone();
        let on = move |ev: Event| {
            let (row, patch) = match ev {
                Event::Started(it) => (index[&it.path], FileRow { name: name_of(&it.path), before: human(it.size).into(), after: "".into(), quality: "".into(), status: "working…".into(), failed: false }),
                Event::Finished(r) => (
                    index[&r.path],
                    FileRow {
                        name: name_of(&r.path),
                        before: human(r.before).into(),
                        after: if r.out.is_some() { human(r.after).into() } else { "".into() },
                        quality: if r.out.is_some() { format!("q{}", r.quality).into() } else { "".into() },
                        status: match &r.error {
                            Some(e) => e.clone().into(),
                            None => format!("{}×{} {}-bit via {}", r.width, r.height, r.bits, r.via).into(),
                        },
                        failed: r.error.is_some(),
                    },
                ),
            };
            let finished = matches!(ev, Event::Finished(_));
            let _ = w.upgrade_in_event_loop(move |ui| {
                ui.get_files().as_any().downcast_ref::<VecModel<FileRow>>().unwrap().set_row_data(row, patch);
                if finished {
                    ui.set_done(ui.get_done() + 1);
                }
            });
        };
        let outs = run_with(&plan, &o, &on, &ctl);
        let impact = Impact::from_outcomes(&outs, &o);
        let recorded = History::open(&History::default_path()).and_then(|mut h| h.record(&o, &outs, &impact));
        let lines: Vec<SharedString> = impact.lines().into_iter().map(Into::into).collect();
        let status = match recorded {
            Ok(id) if ctl.cancel.load(Ordering::Relaxed) => format!("cancelled — {} of {} done, recorded as job #{id}", outs.len(), plan.items.len()),
            Ok(id) => format!("done — job #{id}"),
            Err(e) => format!("done (history not saved: {e:#})"),
        };
        let _ = weak.upgrade_in_event_loop(move |ui| {
            ui.get_impact_lines().as_any().downcast_ref::<VecModel<SharedString>>().unwrap().set_vec(lines);
            ui.set_running(false);
            ui.set_status_line(status.into());
            platform::notify("Husky Forge", &ui.get_impact_lines().iter().take(2).map(|s| s.to_string()).collect::<Vec<_>>().join(" · "));
        });
    });
}
