use super::*;

fn argv(values: &[&str]) -> Vec<OsString> {
    std::iter::once("contextmink")
        .chain(values.iter().copied())
        .map(OsString::from)
        .collect()
}

fn git_bash() -> MsysEnvironment {
    MsysEnvironment {
        msystem: Some("MINGW64".into()),
        no_pathconv: None,
        arg_conv_excl: None,
        path: Some(
            r"C:\Users\Agent\bin;C:\Fake Git\mingw64\bin;C:\Fake Git\usr\bin;C:\Windows\system32"
                .into(),
        ),
    }
}

#[test]
fn refuses_rewritten_positional_and_flag_values() {
    for args in [
        vec![
            "grep",
            "--pattern",
            "C:/Fake Git/skills/contextmink",
            "--literal",
        ],
        vec!["files", "--path-contains", "c:/fake git/contextmink/"],
        vec![
            "json-select",
            "report.json",
            "--at=C:/Fake Git/report/plan_id",
        ],
        vec!["files", "C:/Fake Git/"],
    ] {
        let refusal = rewritten_argument_refusal(&argv(&args), &git_bash())
            .unwrap_or_else(|| panic!("expected refusal for {args:?}"));
        assert!(refusal.contains("MSYS_NO_PATHCONV=1"), "{refusal}");
        assert!(refusal.contains("C:/Fake Git/"), "{refusal}");
    }
    let refusal = rewritten_argument_refusal(
        &argv(&["grep", "--pattern", "C:/Fake Git/skills/contextmink"]),
        &git_bash(),
    )
    .unwrap();
    assert!(refusal.contains("`/skills/contextmink`"), "{refusal}");
}

#[test]
fn allows_ordinary_arguments_under_git_bash() {
    let args = argv(&[
        "grep",
        "--pattern",
        "/skills/contextmink",
        "C:/Users/Agent/project",
        r"C:\Fake Git\usr\bin\bash.exe",
        "--at=/report",
    ]);
    assert_eq!(rewritten_argument_refusal(&args, &git_bash()), None);
}

#[test]
fn explicit_conversion_opt_out_or_non_msys_host_disables_the_check() {
    let rewritten = argv(&["grep", "--pattern", "C:/Fake Git/skills"]);
    let mut opted_out = git_bash();
    opted_out.no_pathconv = Some("1".into());
    assert_eq!(rewritten_argument_refusal(&rewritten, &opted_out), None);

    let mut excluded = git_bash();
    excluded.arg_conv_excl = Some("*".into());
    assert_eq!(rewritten_argument_refusal(&rewritten, &excluded), None);

    let mut native = git_bash();
    native.msystem = None;
    assert_eq!(rewritten_argument_refusal(&rewritten, &native), None);

    let mut empty_msystem = git_bash();
    empty_msystem.msystem = Some(OsString::new());
    assert_eq!(rewritten_argument_refusal(&rewritten, &empty_msystem), None);
}

#[test]
fn only_drive_qualified_usr_bin_entries_define_roots() {
    let environment = MsysEnvironment {
        path: Some("/usr/bin;relative/usr/bin;D:/msys64/usr/bin/".into()),
        ..git_bash()
    };
    assert_eq!(environment.roots(), vec!["D:/msys64/".to_owned()]);
    assert_eq!(
        rewritten_argument_refusal(&argv(&["files", "/etc"]), &environment),
        None
    );
    assert!(rewritten_argument_refusal(&argv(&["files", "D:/msys64/etc"]), &environment).is_some());
}
