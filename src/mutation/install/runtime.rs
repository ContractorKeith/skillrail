//! Durable install publication and conflict-aware recovery.
use super::*;
pub(crate) fn apply(
    app: &Application,
    p: &Plan,
    mut checkpoint: impl FnMut(Checkpoint) -> Result<(), AppError>,
) -> Result<PlanView, AppError> {
    if p.record
        .install
        .as_ref()
        .is_none_or(|i| i.journal != app.paths.state_dir)
    {
        return Err(refused(
            "Install plan belongs to another private journal; use its original --state-dir.",
        ));
    }
    let mut view = p.view.clone();
    if p.undo && p.record.status == "undone" {
        view.status = "unchanged".into();
        return Ok(view);
    }
    if !p.undo {
        let i = p.record.install.as_ref().ok_or_else(journal_error)?;
        let fresh = install_source::collect(&i.request)?;
        if fresh.entries != i.entries || fresh.provenance != i.manifest.provenance {
            return Err(conflict());
        }
        check_names(app, i)?;
        check_destinations(i)?;
    }
    let j = Journal::open(&app.paths)?;
    checkpoint(Checkpoint::Locked)?;
    let mut r = p.record.clone();
    let mut saved = if p.undo {
        let (_, current) = j.load(&r.id)?;
        if Some(&current) != p.record_before.as_ref() {
            return Err(conflict());
        }
        validate_undo(app, &r)?;
        r.status = "undoing".into();
        j.save(&r, Some(&current))?
    } else {
        if j.records()?
            .iter()
            .any(|r| r.status == "prepared" || r.status == "undoing")
        {
            return Err(refused(
                "An interrupted operation requires undo recovery before another apply.",
            ));
        }
        let i = r.install.as_ref().ok_or_else(journal_error)?;
        let fresh = install_source::collect(&i.request)?;
        if fresh.entries != i.entries || fresh.provenance != i.manifest.provenance {
            return Err(conflict());
        }
        check_names(app, i)?;
        check_destinations(i)?;
        j.save(&r, None)?
    };
    checkpoint(Checkpoint::Recorded)?;
    if p.undo {
        let i = r.install.as_ref().ok_or_else(journal_error)?;
        validate_undo(app, &r)?;
        for (n, l) in i.links.iter().enumerate() {
            if let Some(d) = link_dir(l)?
                && let Some(s) = d.link(filename(&l.path)?)?
            {
                if Some(s) != l.published {
                    return Err(conflict());
                }
                d.remove(filename(&l.path)?)?;
            }
            let stage = l.root.path.join(&i.stage);
            if !absent(&stage) {
                let d = Directory::open(&stage)?;
                if d.link("link")?.is_some() {
                    d.remove("link")?;
                }
                Directory::open(&l.root.path)?.remove_dir(&i.stage)?;
            }
            checkpoint(Checkpoint::Published(n + 1))?;
        }
        for path in [&i.manifest.snapshot, &i.store.path.join(&i.stage)] {
            if absent(path) {
                continue;
            }
            check_tree(i, path, true)?;
            let entries = expected(i)?;
            for (n, e) in entries.iter().filter(|e| !e.directory).enumerate() {
                let file = path.join(&e.path);
                if !absent(&file) {
                    check_tree(i, path, true)?;
                    Directory::open(file.parent().ok_or_else(journal_error)?)?
                        .remove(filename(&file)?)?;
                    checkpoint(Checkpoint::TreeFile(n))?;
                }
            }
            let mut dirs = directory_names(&entries);
            dirs.reverse();
            for rel in dirs {
                let d = path.join(rel);
                if !absent(&d) {
                    Directory::open(d.parent().ok_or_else(journal_error)?)?
                        .remove_dir(filename(&d)?)?;
                }
            }
            Directory::open(path.parent().ok_or_else(journal_error)?)?
                .remove_dir(filename(path)?)?;
            checkpoint(Checkpoint::Published(0))?;
        }
    } else {
        let i = r.install.as_mut().ok_or_else(journal_error)?;
        let store = i.store.ensure()?;
        let stage_path = i.store.path.join(&i.stage);
        let stage = store.private_child(&i.stage)?;
        i.tree_chain = Some(stage.chain.clone());
        saved = j.save(&r, Some(&saved))?;
        let i = r.install.as_ref().ok_or_else(journal_error)?;
        let entries = expected(i)?;
        for rel in directory_names(&entries) {
            let path = stage_path.join(&rel);
            let dir = Directory::open(path.parent().ok_or_else(journal_error)?)?
                .private_child(filename(&path)?)?;
            let identity = dir.chain.last().ok_or_else(journal_error)?.clone();
            r.install
                .as_mut()
                .ok_or_else(journal_error)?
                .files
                .insert(rel.to_string_lossy().into_owned(), identity);
            saved = j.save(&r, Some(&saved))?;
        }
        for (n, e) in entries.iter().filter(|e| !e.directory).enumerate() {
            let path = stage_path.join(&e.path);
            let d = Directory::open(path.parent().ok_or_else(journal_error)?)?;
            d.create(
                filename(&path)?,
                &e.bytes,
                if e.executable { 0o700 } else { 0o600 },
                None,
            )?;
            checkpoint(Checkpoint::TreeFile(n))?;
            let s = d.read(filename(&path)?, false)?.ok_or_else(journal_error)?;
            r.install
                .as_mut()
                .ok_or_else(journal_error)?
                .files
                .insert(e.path.clone(), s.identity);
            saved = j.save(&r, Some(&saved))?;
        }
        checkpoint(Checkpoint::Staged(0))?;
        let i = r.install.as_ref().ok_or_else(journal_error)?;
        check_tree(i, &stage_path, false)?;
        check_names(app, i)?;
        check_destinations_after_stage(i)?;
        checkpoint(Checkpoint::BeforePublish(0))?;
        check_tree(i, &stage_path, false)?;
        stage_parent(i)?.replace(&i.stage, filename(&i.manifest.snapshot)?, true)?;
        checkpoint(Checkpoint::Published(0))?;
        for n in 0..r.install.as_ref().ok_or_else(journal_error)?.links.len() {
            let i = r.install.as_mut().ok_or_else(journal_error)?;
            let l = &mut i.links[n];
            let d = l.root.ensure()?;
            l.parents = Some(d.chain.clone());
            // Save parent binding before creating the link stage.
            saved = j.save(&r, Some(&saved))?;
            let i = r.install.as_ref().ok_or_else(journal_error)?;
            let l = &i.links[n];
            let d = Directory::open(&l.root.path)?;
            let stage = d.private_child(&i.stage)?;
            stage.symlink("link", &i.manifest.snapshot)?;
            let link = stage.link("link")?.ok_or_else(journal_error)?;
            r.install.as_mut().ok_or_else(journal_error)?.links[n].published = Some(link);
            saved = j.save(&r, Some(&saved))?;
            checkpoint(Checkpoint::Staged(n + 1))?;
            let i = r.install.as_ref().ok_or_else(journal_error)?;
            let l = &i.links[n];
            if link_dir(l)?.is_none() || stage.link("link")? != l.published || !absent(&l.path) {
                return Err(conflict());
            }
            checkpoint(Checkpoint::BeforePublish(n + 1))?;
            check_names(app, i)?;
            let d = link_dir(l)?.ok_or_else(conflict)?;
            let current_stage = Directory::open(&l.root.path.join(&i.stage))?;
            if current_stage.chain != stage.chain || current_stage.link("link")? != l.published {
                return Err(conflict());
            }
            if stage.link("link")? != l.published {
                return Err(conflict());
            }
            stage.replace_into("link", &d, filename(&l.path)?, true)?;
            d.remove_dir(&i.stage)?;
            checkpoint(Checkpoint::Published(n + 1))?;
            if link_dir(l)?
                .ok_or_else(conflict)?
                .link(filename(&l.path)?)?
                != l.published
            {
                return Err(conflict());
            }
        }
    }
    r.status = if p.undo { "undone" } else { "applied" }.into();
    j.save(&r, Some(&saved))?;
    view.status = r.status;
    Ok(view)
}
fn stage_parent(i: &Install) -> Result<Directory, AppError> {
    i.store.check()?;
    Directory::open(&i.store.path)
}
fn check_destinations_after_stage(i: &Install) -> Result<(), AppError> {
    i.store.check()?;
    if !absent(&i.manifest.snapshot) {
        return Err(conflict());
    }
    for l in &i.links {
        l.root.check()?;
        if !absent(&l.path) {
            return Err(conflict());
        }
    }
    Ok(())
}
