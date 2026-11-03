//! Durable original retention, replacement publication and restartable restoration.
use super::*;
fn backup_tree(l: &Lifecycle) -> PathBuf {
    l.old.store.path.join(&l.backup)
}
fn backup_link(l: &Lifecycle, n: usize) -> PathBuf {
    l.old.links[n].root.path.join(&l.backup)
}
fn check_backup_link(l: &Lifecycle, n: usize) -> Result<Option<Snapshot>, AppError> {
    let path = backup_link(l, n);
    if absent(&path) {
        return Ok(None);
    }
    let d = Directory::open(&path)?;
    if l.link_chains[n].as_ref().is_some_and(|c| c != &d.chain) {
        return Err(conflict());
    }
    let link = d.link("link")?;
    if link.is_some() && link != l.old.links[n].published {
        return Err(conflict());
    }
    if fs::read_dir(path).map_err(|_| conflict())?.count() != usize::from(link.is_some()) {
        return Err(conflict());
    }
    Ok(link)
}
pub(in crate::mutation::install) fn validate_undo(
    app: &Application,
    r: &Record,
) -> Result<(), AppError> {
    let l = r.lifecycle.as_ref().ok_or_else(journal_error)?;
    if l.old.journal != app.paths.state_dir {
        return Err(conflict());
    }
    if r.status == "undone" {
        return Ok(());
    }
    l.old.store.check()?;
    validate_consumers(app, &l.old)?;
    let backup = backup_tree(l);
    validate_consumers_at(app, &l.old, &backup)?;
    if r.status == "applied" {
        if absent(&backup) || !absent(&l.old.manifest.snapshot) {
            return Err(conflict());
        }
        for (n, link) in l.old.links.iter().enumerate() {
            if check_backup_link(l, n)? != link.published {
                return Err(conflict());
            }
            let current = link_dir(link)?
                .ok_or_else(conflict)?
                .link(filename(&link.path)?)?;
            let expected = r
                .install
                .as_ref()
                .and_then(|i| i.links[n].published.clone());
            if current != expected {
                return Err(conflict());
            }
        }
        if let Some(i) = &r.install {
            check_tree(i, &i.manifest.snapshot, false)?;
        }
    }
    if !absent(&backup) {
        check_tree(&l.old, &backup, false)?;
    } else {
        check_tree(&l.old, &l.old.manifest.snapshot, false)?;
    }
    for (n, link) in l.old.links.iter().enumerate() {
        let held = check_backup_link(l, n)?;
        let current = link_dir(link)?
            .ok_or_else(conflict)?
            .link(filename(&link.path)?)?;
        let new = r
            .install
            .as_ref()
            .and_then(|i| i.links.get(n))
            .and_then(|l| l.published.as_ref());
        if current != link.published
            && !(held.is_some() && (current.is_none() || current.as_ref() == new))
        {
            return Err(conflict());
        }
    }
    if let Some(i) = &r.install {
        validate_consumers(app, i)?;
        validate_consumers_at(app, i, &i.store.path.join(&i.stage))?;
        for link in &i.links {
            let stage = link.root.path.join(&i.stage);
            if !absent(&stage) {
                let d = Directory::open(&stage)?;
                let staged = d.link("link")?;
                if staged.as_ref().is_some_and(|s| {
                    s.bytes != i.manifest.snapshot.to_string_lossy().as_bytes()
                        || link.published.as_ref().is_some_and(|p| p != s)
                }) || fs::read_dir(stage).map_err(|_| conflict())?.count()
                    != usize::from(staged.is_some())
                {
                    return Err(conflict());
                }
            }
        }
        if i.manifest.snapshot != l.old.manifest.snapshot {
            if !absent(&i.manifest.snapshot) {
                check_tree(i, &i.manifest.snapshot, r.status == "undoing")?;
            }
            let stage = i.store.path.join(&i.stage);
            if !absent(&stage) {
                check_tree(i, &stage, true)?;
            }
        }
    }
    Ok(())
}
pub(in crate::mutation) fn plan_undo(
    app: &Application,
    r: Record,
    before: Snapshot,
) -> Result<Plan, AppError> {
    validate_undo(app, &r)?;
    plan(r, true, Some(before))
}
pub(in crate::mutation) fn apply(
    app: &Application,
    p: &Plan,
    mut checkpoint: impl FnMut(Checkpoint) -> Result<(), AppError>,
) -> Result<PlanView, AppError> {
    let mut view = p.view.clone();
    let l = p.record.lifecycle.as_ref().ok_or_else(journal_error)?;
    check_journal(app, &l.old)?;
    if p.undo && p.record.status == "undone" {
        view.status = "unchanged".into();
        return Ok(view);
    }
    if !p.undo {
        original(app, &l.old)?;
        revalidate_source(&p.record)?;
    }
    let j = Journal::open(&app.paths)?;
    checkpoint(Checkpoint::Locked)?;
    let mut r = p.record.clone();
    let mut saved;
    if p.undo {
        let (_, current) = j.load(&r.id)?;
        if Some(&current) != p.record_before.as_ref() {
            return Err(conflict());
        }
        validate_undo(app, &r)?;
        r.status = "undoing".into();
        saved = j.save(&r, Some(&current))?;
    } else {
        if j.records()?
            .iter()
            .any(|r| ["prepared", "undoing"].contains(&r.status.as_str()))
        {
            return Err(refused(
                "An interrupted operation requires undo before another apply.",
            ));
        }
        original(app, &l.old)?;
        revalidate_source(&r)?;
        if l.noop {
            view.status = "unchanged".into();
            return Ok(view);
        }
        reserve_links(l, r.install.is_some())?;
        saved = j.save(&r, None)?;
    }
    checkpoint(Checkpoint::Recorded)?;
    if p.undo {
        restore(app, &mut r, &mut checkpoint)?;
    } else {
        retain(app, &mut r, &j, &mut saved, &mut checkpoint)?;
        if r.install.is_some() {
            runtime::publish(app, &mut r, &j, &mut saved, &mut checkpoint)?;
        }
        validate_undo(app, &r)?;
    }
    r.status = if p.undo { "undone" } else { "applied" }.into();
    j.save(&r, Some(&saved))?;
    view.status = r.status;
    Ok(view)
}
pub(in crate::mutation::install) fn revalidate_source(r: &Record) -> Result<(), AppError> {
    let l = r.lifecycle.as_ref().ok_or_else(journal_error)?;
    if r.install.is_some() && source_chain(&l.old)? != l.source_chain {
        return Err(conflict());
    }
    if let Some(i) = &r.install {
        let fresh = install_source::collect(&i.request)?;
        if fresh.entries != i.entries || fresh.provenance != i.manifest.provenance {
            return Err(conflict());
        }
    }
    Ok(())
}
fn retain(
    app: &Application,
    r: &mut Record,
    j: &Journal,
    saved: &mut Snapshot,
    checkpoint: &mut impl FnMut(Checkpoint) -> Result<(), AppError>,
) -> Result<(), AppError> {
    let count = r
        .lifecycle
        .as_ref()
        .ok_or_else(journal_error)?
        .old
        .links
        .len();
    for n in 0..count {
        let l = r.lifecycle.as_ref().ok_or_else(journal_error)?;
        let link = &l.old.links[n];
        let dir = link_dir(link)?.ok_or_else(conflict)?;
        if !absent(&backup_link(l, n)) {
            return Err(conflict());
        }
        let stage = dir.private_child(&l.backup)?;
        r.lifecycle.as_mut().ok_or_else(journal_error)?.link_chains[n] = Some(stage.chain.clone());
        *saved = j.save(r, Some(saved))?;
        checkpoint(Checkpoint::BeforeRetain(n + 1))?;
        let l = r.lifecycle.as_ref().ok_or_else(journal_error)?;
        validate_consumers(app, &l.old)?;
        check_tree(&l.old, &l.old.manifest.snapshot, false)?;
        let link = &l.old.links[n];
        let dir = link_dir(link)?.ok_or_else(conflict)?;
        if dir.link(filename(&link.path)?)? != link.published
            || check_backup_link(l, n)?.is_some()
            || Directory::open(&backup_link(l, n))?.chain != stage.chain
        {
            return Err(conflict());
        }
        dir.replace_into(filename(&link.path)?, &stage, "link", true)?;
        checkpoint(Checkpoint::Retained(n + 1))?;
    }
    checkpoint(Checkpoint::BeforeRetain(0))?;
    let l = r.lifecycle.as_ref().ok_or_else(journal_error)?;
    validate_consumers(app, &l.old)?;
    check_tree(&l.old, &l.old.manifest.snapshot, false)?;
    if !absent(&backup_tree(l)) {
        return Err(conflict());
    }
    Directory::open(&l.old.store.path)?.replace(
        filename(&l.old.manifest.snapshot)?,
        &l.backup,
        true,
    )?;
    check_tree(&l.old, &backup_tree(l), false)?;
    checkpoint(Checkpoint::Retained(0))?;
    Ok(())
}
fn restore(
    app: &Application,
    r: &mut Record,
    checkpoint: &mut impl FnMut(Checkpoint) -> Result<(), AppError>,
) -> Result<(), AppError> {
    validate_undo(app, r)?;
    if r.install.is_some() {
        let l = r.lifecycle.as_ref().ok_or_else(journal_error)?;
        if !absent(&backup_tree(l)) {
            let mut new = r.clone();
            new.version = 3;
            new.lifecycle = None;
            // Once an old link is back, its new counterpart has already been removed.
            // Cleanup only the new paths that remain, never the restored original.
            let i = new.install.as_mut().ok_or_else(journal_error)?;
            let mut remaining = vec![];
            for link in i.links.drain(..) {
                let current = link_dir(&link)?
                    .ok_or_else(conflict)?
                    .link(filename(&link.path)?)?;
                let old = l
                    .old
                    .links
                    .iter()
                    .find(|old| old.path == link.path)
                    .ok_or_else(journal_error)?;
                if current != old.published {
                    remaining.push(link);
                }
            }
            i.links = remaining;
            runtime::cleanup(app, &new, &mut |point| {
                checkpoint(point)?;
                validate_undo(app, r)
            })?;
        }
    }
    let l = r.lifecycle.as_ref().ok_or_else(journal_error)?;
    validate_undo(app, r)?;
    if !absent(&backup_tree(l)) {
        if !absent(&l.old.manifest.snapshot) {
            return Err(conflict());
        }
        checkpoint(Checkpoint::BeforeRestore(0))?;
        validate_undo(app, r)?;
        Directory::open(&l.old.store.path)?.replace(
            &l.backup,
            filename(&l.old.manifest.snapshot)?,
            true,
        )?;
        checkpoint(Checkpoint::Restored(0))?;
    }
    for (n, link) in l.old.links.iter().enumerate() {
        if check_backup_link(l, n)?.is_some() {
            checkpoint(Checkpoint::BeforeRestore(n + 1))?;
            validate_undo(app, r)?;
            let dir = link_dir(link)?.ok_or_else(conflict)?;
            if dir.link(filename(&link.path)?)?.is_some() {
                return Err(conflict());
            }
            Directory::open(&backup_link(l, n))?.replace_into(
                "link",
                &dir,
                filename(&link.path)?,
                true,
            )?;
            checkpoint(Checkpoint::Restored(n + 1))?;
        }
        let backup = backup_link(l, n);
        if !absent(&backup) {
            check_backup_link(l, n)?;
            Directory::open(&link.root.path)?.remove_dir(&l.backup)?;
        }
    }
    original(app, &l.old)
}
