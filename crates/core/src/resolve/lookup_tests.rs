use super::*;
use crate::request::{CollectionDefaults, Environment, FolderDefaults, Request, Var};

fn var(n: &str, v: &str) -> Var {
    Var {
        name: n.into(),
        value: v.into(),
    }
}

fn lk(
    r: &Request,
    c: &CollectionDefaults,
    f: &[FolderDefaults],
    e: Option<&Environment>,
    n: &str,
) -> VarLookup {
    lookup_var(
        r,
        &Scope {
            collection: c,
            folders: f.iter().collect(),
            environment: e,
        },
        n,
    )
}

fn found(l: &VarLookup) -> (&str, &VarSource, bool) {
    match l {
        VarLookup::Found {
            value,
            source,
            secret,
        } => (value, source, *secret),
        other => panic!("not found: {other:?}"),
    }
}

#[test]
fn precedence_chain() {
    let mut r = Request::default();
    let mut c = CollectionDefaults::default();
    let f0 = FolderDefaults {
        vars: vec![var("x", "outer")],
        ..Default::default()
    };
    let f1 = FolderDefaults {
        vars: vec![var("x", "inner")],
        ..Default::default()
    };
    let env = Environment {
        vars: vec![var("x", "env")],
        secrets: vec![var("x", "sec")],
        ..Default::default()
    };
    c.vars = vec![var("x", "coll")];
    let folders = [f0, f1];
    r.vars = vec![var("x", "req")];
    let l = lk(&r, &c, &folders, Some(&env), "x");
    assert_eq!((found(&l).0, found(&l).1), ("req", &VarSource::Request));
    r.vars.clear();
    let l = lk(&r, &c, &folders, Some(&env), "x");
    assert_eq!((found(&l).0, found(&l).1), ("inner", &VarSource::Folder(1)));
    let l = lk(&r, &c, &folders[..1], Some(&env), "x");
    assert_eq!(found(&l).1, &VarSource::Folder(0));
    let l = lk(&r, &c, &[], Some(&env), "x");
    assert_eq!(
        (found(&l).0, found(&l).1, found(&l).2),
        ("sec", &VarSource::Secret, true)
    );
    let env2 = Environment {
        vars: vec![var("x", "env")],
        ..Default::default()
    };
    let l = lk(&r, &c, &[], Some(&env2), "x");
    assert_eq!((found(&l).0, found(&l).1), ("env", &VarSource::Environment));
    let l = lk(&r, &c, &[], None, "x");
    assert_eq!((found(&l).0, found(&l).1), ("coll", &VarSource::Collection));
}

#[test]
fn dynamic_and_unresolved() {
    let (r, c) = (Request::default(), CollectionDefaults::default());
    assert_eq!(lk(&r, &c, &[], None, "$uuid"), VarLookup::Dynamic);
    assert_eq!(lk(&r, &c, &[], None, "$timestamp"), VarLookup::Dynamic);
    assert_eq!(lk(&r, &c, &[], None, "nope"), VarLookup::Unresolved);
    assert_eq!(lk(&r, &c, &[], None, "$other"), VarLookup::Unresolved);
}

#[test]
fn name_trimming_matches_resolve() {
    let r = Request {
        vars: vec![var("a", "1")],
        ..Default::default()
    };
    let c = CollectionDefaults::default();
    assert_eq!(found(&lk(&r, &c, &[], None, " a ")).0, "1");
}

#[test]
fn expands_nested_and_tracks_secret() {
    let r = Request {
        vars: vec![var("a", "x-{{s}}")],
        ..Default::default()
    };
    let c = CollectionDefaults::default();
    let env = Environment {
        secrets: vec![var("s", "hunter2")],
        ..Default::default()
    };
    let l = lk(&r, &c, &[], Some(&env), "a");
    assert_eq!(found(&l).0, "x-hunter2");
    assert!(found(&l).2);
    assert!(!format!("{l:?}").contains("hunter2"));
}

#[test]
fn cycle_is_an_error() {
    let r = Request {
        vars: vec![var("a", "{{b}}"), var("b", "{{a}}")],
        ..Default::default()
    };
    let c = CollectionDefaults::default();
    assert!(matches!(
        lk(&r, &c, &[], None, "a"),
        VarLookup::Error(Problem::VarCycle { .. })
    ));
}
