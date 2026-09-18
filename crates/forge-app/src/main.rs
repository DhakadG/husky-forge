#![cfg_attr(windows, windows_subsystem = "windows")]
//! Husky Forge desktop app: Slint front end over forge-core.
mod launch;
mod platform;
mod presets;
mod update;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result};
use forge_core::{Control, Event, Format, History, Impact, MetaMode, Mode, Options, Plan, RawLook, Resize, impact::human, output_dir_for, plan, run_with};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

slint::include_modules!();

#[derive(Default)]
pub struct State {
    paths: Vec<PathBuf>,
    plan: Option<Arc<Plan>>,
    ctl: Arc<Control>,
    /// Pending OS file drops (winit delivers one path per event; the drop ends when hovering stops).
    pub dropping: Vec<PathBuf>,
    last_output: Option<PathBuf>,
    log_path: PathBuf,
}

thread_local! {
    // The UI thread's state, reachable from `upgrade_in_event_loop` closures (which cannot capture an Rc).
    static STATE: RefCell<Option<Rc<RefCell<State>>>> = const { RefCell::new(None) };
}

const LOG_ROWS: usize = 300;

fn main() -> Result<()> {
    if launch::forward_to_running() {
        return Ok(());
    }
    // Log file + a live tap feeding the in-app log panel.
    let tap: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let log_path = forge_core::logging::init(Some(Box::new({
        let tap = tap.clone();
        move |line| {
            if let Ok(mut v) = tap.lock() {
                v.push(line.to_string());
            }
        }
    })));
    log::info!("app start v{}", env!("CARGO_PKG_VERSION"));

    let ui = App::new()?;
    platform::decorate(&ui);
    let st = Rc::new(RefCell::new(State { log_path, ..Default::default() }));
    STATE.with(|s| *s.borrow_mut() = Some(st.clone()));
    ui.set_files(ModelRc::new(VecModel::<FileRow>::default()));
    ui.set_impact_lines(ModelRc::new(VecModel::<SharedString>::default()));
    ui.set_log_lines(ModelRc::new(VecModel::<SharedString>::default()));
    ui.set_presets(ModelRc::new(VecModel::from(presets::list())));

    // Drain the log tap into the panel four times a second.
    let log_timer = slint::Timer::default();
    log_timer.start(slint::TimerMode::Repeated, std::time::Duration::from_millis(250), {
        let (ui, tap) = (ui.as_weak(), tap.clone());
        move || {
            let Ok(mut v) = tap.lock() else { return };
            if v.is_empty() {
                return;
            }
            let fresh: Vec<String> = v.drain(..).collect();
            drop(v);
            let ui = ui.unwrap();
            let m = ui.get_log_lines();
            let m = m.as_any().downcast_ref::<VecModel<SharedString>>().unwrap();
            for l in fresh {
                m.push(l.into());
            }
            while m.row_count() > LOG_ROWS {
                m.remove(0);
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
    platform::hook_os_drops(&ui, st.clone());

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
    ui.on_clear({
        let (ui, st) = (ui.as_weak(), st.clone());
        move || {
            st.borrow_mut().paths.clear();
            replan(&ui.unwrap(), &st);
        }
    });
    ui.on_pick_lut({
        let (ui, st) = (ui.as_weak(), st.clone());
        move || {
            if let Some(p) = rfd::FileDialog::new().add_filter("LUT", &["cube"]).pick_file() {
                let ui = ui.unwrap();
                ui.set_lut_path(p.to_string_lossy().to_string().into());
                replan(&ui, &st);
            }
        }
    });
    ui.on_pick_output_dir({
        let (ui, st) = (ui.as_weak(), st.clone());
        move || {
            if let Some(p) = rfd::FileDialog::new().set_title("Output folder").pick_folder() {
                let ui = ui.unwrap();
                ui.set_output_dir(p.to_string_lossy().to_string().into());
                ui.set_output_index(1);
                replan(&ui, &st);
            }
        }
    });
    ui.on_open_output({
        let st = st.clone();
        move || {
            if let Some(p) = st.borrow().last_output.clone() {
                let _ = open::that_detached(p);
            }
        }
    });
    ui.on_open_log({
        let st = st.clone();
        move || {
            let _ = open::that_detached(st.borrow().log_path.clone());
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
            log::info!("undo: {msg:?}");
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
    ui.on_start_job({
        let (ui, st) = (ui.as_weak(), st.clone());
        move || start(&ui.unwrap(), &st)
    });

    // Explorer / Finder / second instances arrive as a `Launch`: preset + flags + paths (+ --start).
    apply_launch(&ui, &st, launch::from_env());
    launch::listen({
        let weak = ui.as_weak();
        move |l| {
            let _ = weak.upgrade_in_event_loop(move |ui| {
                if let Some(st) = STATE.with(|s| s.borrow().clone()) {
                    apply_launch(&ui, &st, l);
                }
            });
        }
    });

    // Version check off the UI thread; a newer tag just shows in the header.
    {
        let weak = ui.as_weak();
        std::thread::spawn(move || {
            if let Some(tag) = update::newer_release() {
                let _ = weak.upgrade_in_event_loop(move |ui| ui.set_update_line(format!("{tag} available — {}", update::RELEASES).into()));
            }
        });
    }

    ui.run()?;
    Ok(())
}

pub fn add(ui: &App, st: &Rc<RefCell<State>>, new: Vec<PathBuf>) {
    let mut s = st.borrow_mut();
    for p in new {
        if p.exists() && !s.paths.contains(&p) {
            s.paths.push(p);
        }
    }
    drop(s);
    replan(ui, st);
}

/// Preset, then flag overrides, then paths; `--start` runs immediately (Explorer one-click actions).
fn apply_launch(ui: &App, st: &Rc<RefCell<State>>, l: launch::Launch) {
    if let Some(name) = &l.preset {
        match presets::load(name) {
            Ok(o) => apply_options(ui, &o),
            Err(e) => ui.set_status_line(format!("preset: {e:#}").into()),
        }
    }
    if let Some(f) = l.to {
        ui.set_format_index(f as i32);
    }
    if let Some(q) = l.quality {
        ui.set_quality(q.clamp(1, 100) as f32);
    }
    if let Some(mp) = l.max_mp {
        ui.set_resize_index(if mp > 0.0 { 1 } else { 0 });
        ui.set_max_mp(format!("{mp}").into());
    }
    if let Some(mb) = l.target_mb {
        ui.set_target_mb(format!("{mb}").into());
    }
    if let Some(m) = l.meta {
        ui.set_meta_index(m as i32);
    }
    if let Some(m) = l.mode {
        ui.set_originals_index(m as i32);
    }
    if let Some(out) = &l.out {
        ui.set_output_dir(out.to_string_lossy().to_string().into());
        ui.set_output_index(1);
    }
    let paths: Vec<PathBuf> = l.paths.into_iter().filter(|p| p.exists()).collect();
    if paths.is_empty() {
        replan(ui, st);
    } else {
        add(ui, st, paths);
    }
    if l.start && !ui.get_running() && ui.get_file_count() > 0 {
        start(ui, st);
    }
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
    let out = ui.get_output_dir().trim().to_string();
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
        output_dir: (ui.get_output_index() == 1 && !out.is_empty()).then(|| PathBuf::from(out)),
        raw_look: if ui.get_raw_flat() { RawLook::Flat } else { RawLook::Auto },
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
    ui.set_output_index(o.output_dir.is_some() as i32);
    ui.set_output_dir(o.output_dir.as_ref().map(|p| p.to_string_lossy().to_string()).unwrap_or_default().into());
    ui.set_raw_flat(o.raw_look == RawLook::Flat);
    ui.set_include_small(o.min_bytes == 0);
    ui.set_skip_sidecar(o.skip_sidecar);
    ui.set_only_if_smaller(o.only_if_smaller);
    ui.set_workers(o.workers.to_string().into());
}

/// Re-walk the sources with the current options; show the estimate and the planned file list.
pub fn replan(ui: &App, st: &Rc<RefCell<State>>) {
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
    ui.set_output_hint(match s.paths.first() {
        Some(root) => format!("→ {}{}", output_dir_for(root, &o).display(), if s.paths.len() > 1 { " …" } else { "" }).into(),
        None => "".into(),
    });
    ui.set_status_line(if p.skipped.is_empty() { "".into() } else { format!("{} skipped — {}", p.skipped.len(), p.skipped[0].1).into() });
    let lines: Vec<SharedString> = if p.items.is_empty() { vec![] } else { Impact::estimate(&p, &o).lines().into_iter().map(Into::into).collect() };
    ui.get_impact_lines().as_any().downcast_ref::<VecModel<SharedString>>().unwrap().set_vec(lines);
    let rows: Vec<FileRow> = p
        .items
        .iter()
        .map(|i| FileRow { name: name_of(&i.path), before: human(i.size).into(), after: "".into(), quality: "".into(), status: "planned".into(), failed: false, working: false })
        .chain(p.skipped.iter().map(|(path, why)| FileRow { name: name_of(path), before: "".into(), after: "".into(), quality: "".into(), status: format!("skip: {why}").into(), failed: false, working: false }))
        .collect();
    ui.get_files().as_any().downcast_ref::<VecModel<FileRow>>().unwrap().set_vec(rows);
    ui.set_done(0);
    ui.set_failed(0);
    ui.set_total(p.items.len() as i32);
    ui.set_finished(false);
    ui.set_progress_line(if p.items.is_empty() { "".into() } else { format!("{} files planned", p.items.len()).into() });
    s.plan = Some(Arc::new(p));
}

fn name_of(p: &Path) -> SharedString {
    p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default().into()
}

fn fmt_secs(s: u64) -> String {
    if s >= 3600 {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
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
    {
        let mut s = st.borrow_mut();
        s.ctl = ctl.clone();
        s.last_output = s.paths.first().map(|r| output_dir_for(r, &o));
    }
    let index: HashMap<PathBuf, usize> = plan.items.iter().enumerate().map(|(i, it)| (it.path.clone(), i)).collect();
    ui.set_running(true);
    ui.set_paused(false);
    ui.set_finished(false);
    ui.set_done(0);
    ui.set_failed(0);
    ui.set_status_line("".into());
    let t0 = Instant::now();
    let total = plan.items.len();
    let weak = ui.as_weak();
    std::thread::spawn(move || {
        let w = weak.clone();
        let on = move |ev: Event| {
            let (row, patch, finished_ok) = match ev {
                Event::Started(it) => (
                    index[&it.path],
                    FileRow { name: name_of(&it.path), before: human(it.size).into(), after: "".into(), quality: "".into(), status: "working…".into(), failed: false, working: true },
                    None,
                ),
                Event::Finished(r) => (
                    index[&r.path],
                    FileRow {
                        name: name_of(&r.path),
                        before: human(r.before).into(),
                        after: if r.out.is_some() { human(r.after).into() } else { "".into() },
                        quality: if r.out.is_some() { format!("q{}", r.quality).into() } else { "".into() },
                        status: match &r.error {
                            Some(e) => e.clone().into(),
                            None => format!("{}×{} {}-bit via {} · {:.1}s", r.width, r.height, r.bits, r.via, r.ms as f64 / 1000.0).into(),
                        },
                        failed: r.error.is_some(),
                        working: false,
                    },
                    Some(r.error.is_none()),
                ),
            };
            let _ = w.upgrade_in_event_loop(move |ui| {
                ui.get_files().as_any().downcast_ref::<VecModel<FileRow>>().unwrap().set_row_data(row, patch);
                if let Some(ok) = finished_ok {
                    let done = ui.get_done() + 1;
                    ui.set_done(done);
                    if !ok {
                        ui.set_failed(ui.get_failed() + 1);
                    }
                    let el = t0.elapsed().as_secs();
                    let left = el * (total as u64 - done as u64) / done as u64;
                    let failed = ui.get_failed();
                    let failed_txt = if failed > 0 { format!(" · {failed} failed") } else { String::new() };
                    ui.set_progress_line(format!("{done} / {total}{failed_txt} · {} · ~{} left", fmt_secs(el), fmt_secs(left)).into());
                }
            });
        };
        let outs = run_with(&plan, &o, &on, &ctl);
        let impact = Impact::from_outcomes(&outs, &o);
        let recorded = History::open(&History::default_path()).and_then(|mut h| h.record(&o, &outs, &impact));
        let lines: Vec<SharedString> = impact.lines().into_iter().map(Into::into).collect();
        let cancelled = ctl.cancel.load(Ordering::Relaxed);
        let status = match recorded {
            Ok(id) if cancelled => format!("cancelled — {} of {} done, recorded as job #{id}", outs.len(), total),
            Ok(id) => format!("done in {} — job #{id}", fmt_secs(t0.elapsed().as_secs())),
            Err(e) => format!("done (history not saved: {e:#})"),
        };
        let _ = weak.upgrade_in_event_loop(move |ui| {
            ui.get_impact_lines().as_any().downcast_ref::<VecModel<SharedString>>().unwrap().set_vec(lines);
            ui.set_running(false);
            ui.set_finished(true);
            ui.set_status_line(status.into());
            let head = ui.get_impact_lines().iter().take(2).map(|s| s.to_string()).collect::<Vec<_>>().join(" · ");
            platform::notify("Husky Forge", &head);
        });
    });
}
