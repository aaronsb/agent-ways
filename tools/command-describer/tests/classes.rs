//! The class rules and subcommand reading, each checked on real commands.

use command_describer::{parse, Class, Role};

fn class(cmd: &str) -> Option<Class> {
    parse(cmd)[0].class
}

fn sub(cmd: &str) -> Option<String> {
    parse(cmd)[0].subcommand.clone()
}

#[test]
fn gh_classes() {
    assert_eq!(class("gh pr create --fill"), Some(Class::Publish));
    assert_eq!(class("gh pr merge 12 --merge"), Some(Class::Publish));
    assert_eq!(class("gh issue comment 5 --body x"), Some(Class::Publish));
    assert_eq!(class("gh pr view 12 --json state"), Some(Class::VcsRead));
    assert_eq!(
        class("gh api -X POST repos/o/r/issues -f title=x"),
        Some(Class::Publish)
    );
    assert_eq!(class("gh api repos/o/r/pulls"), Some(Class::VcsRead));
    assert_eq!(class("gh run rerun 123 --failed"), Some(Class::Publish));
    assert_eq!(class("gh run view 123 --log"), Some(Class::VcsRead));
    assert_eq!(class("gh -R owner/repo pr create"), Some(Class::Publish));
}

#[test]
fn git_classes() {
    assert_eq!(class("git push origin main"), Some(Class::Publish));
    assert_eq!(class("git commit -m x"), Some(Class::VcsWrite));
    assert_eq!(class("git stash pop"), Some(Class::VcsWrite));
    assert_eq!(class("git stash list"), Some(Class::VcsRead));
    assert_eq!(class("git worktree list"), Some(Class::VcsRead));
    assert_eq!(class("git worktree add ../x"), Some(Class::VcsWrite));
    assert_eq!(class("git remote -v"), Some(Class::VcsRead));
    assert_eq!(class("git remote add origin url"), Some(Class::VcsWrite));
    assert_eq!(class("git config user.name"), Some(Class::VcsRead));
    assert_eq!(
        class("git config --global user.name x"),
        Some(Class::VcsWrite)
    );
    assert_eq!(class("git branch -D x"), Some(Class::VcsWrite));
    assert_eq!(class("git branch --show-current"), Some(Class::VcsRead));
    assert_eq!(class("git grep -n foo"), Some(Class::Search));
    assert_eq!(class("git -C tools status"), Some(Class::VcsRead));
}

#[test]
fn other_classes() {
    assert_eq!(
        class("systemctl --user restart attend"),
        Some(Class::Service)
    );
    assert_eq!(class("systemctl status nginx"), Some(Class::ReadInspect));
    assert_eq!(class("docker ps -a"), Some(Class::ReadInspect));
    assert_eq!(class("pkill -f server"), Some(Class::Service));
    assert_eq!(class("pacman -Syu"), Some(Class::Package));
    assert_eq!(class("pacman -Ss foo"), Some(Class::ReadInspect));
    assert_eq!(class("npm install"), Some(Class::Package));
    assert_eq!(class("npm test"), Some(Class::BuildTest));
    assert_eq!(class("pip list"), Some(Class::ReadInspect));
    assert_eq!(class("go test ./..."), Some(Class::BuildTest));
    assert_eq!(class("go env GOPATH"), Some(Class::ReadInspect));
    assert_eq!(class("go get x"), Some(Class::Package));
    assert_eq!(class("cargo add serde"), Some(Class::Package));
    assert_eq!(class("cargo clippy --workspace"), Some(Class::BuildTest));
    assert_eq!(class("node script.js"), Some(Class::Other));
    assert_eq!(class("node --test"), Some(Class::BuildTest));
    assert_eq!(class("rg -n foo src"), Some(Class::Search));
    assert_eq!(
        class("find . -name x -exec rm {} \\;"),
        Some(Class::FileWrite)
    );
    assert_eq!(class("find . -exec grep -l x {} +"), Some(Class::Search));
    assert_eq!(
        class("find . -name '*.tmp' -delete"),
        Some(Class::FileWrite)
    );
    assert_eq!(class("sed -n 1p f"), Some(Class::ReadInspect));
    assert_eq!(class("tar -tzf a.tgz"), Some(Class::ReadInspect));
    assert_eq!(class("tar -xzf a.tgz"), Some(Class::FileWrite));
    assert_eq!(class("rm -rf target"), Some(Class::FileWrite));
    assert_eq!(class("ls -la"), Some(Class::ReadInspect));
    assert_eq!(class("adr new ways \"Title\""), Some(Class::FileWrite));
    assert_eq!(class("ways corpus build"), Some(Class::BuildTest));
    assert_eq!(class("attend message send x"), Some(Class::Publish));
    assert_eq!(class("curl https://example.com"), Some(Class::Other));
}

#[test]
fn subcommand_depths_follow_the_table() {
    assert_eq!(
        sub("docker compose -f x.yml up -d").as_deref(),
        Some("compose up")
    );
    assert_eq!(sub("kubectl -n foo get pods").as_deref(), Some("get"));
    assert_eq!(sub("make -C tools -j4 test").as_deref(), Some("test"));
    assert_eq!(
        sub("git worktree add ../x -b y").as_deref(),
        Some("worktree add")
    );
    assert_eq!(sub("gh api repos/o/r").as_deref(), Some("api"));
    assert_eq!(
        sub("ways author lint --check").as_deref(),
        Some("author lint")
    );
    assert_eq!(
        sub("systemctl --user restart x").as_deref(),
        Some("restart")
    );
    assert_eq!(sub("make").as_deref(), None);
    assert_eq!(sub("grep -rn foo src"), None);
}

#[test]
fn a_redirect_to_a_file_is_an_operative_write() {
    // Review finding 5: navigation and filter programs that redirect write.
    let segs = parse("echo x >> ~/.bashrc");
    assert_eq!(
        (segs[0].role, segs[0].class),
        (Role::Operative, Some(Class::FileWrite))
    );
    let segs = parse("curl -s https://x | jq . > out.json");
    assert_eq!(
        (segs[1].role, segs[1].class),
        (Role::Operative, Some(Class::FileWrite))
    );
    let segs = parse("> empty.txt");
    assert_eq!(
        (segs[0].program.as_deref(), segs[0].class),
        (None, Some(Class::FileWrite))
    );
    // A more consequential class stands: the redirect only raises a class.
    assert_eq!(class("cargo test > log"), Some(Class::BuildTest));
    assert_eq!(class("git push > log 2>&1"), Some(Class::Publish));
    // Not a file: /dev/null and fd duplication.
    assert_eq!(parse("echo x >&2")[0].role, Role::Navigation);
    assert_eq!(class("ls 2>/dev/null"), Some(Class::ReadInspect));
}

#[test]
fn publish_rules_for_registries_and_the_forge() {
    // Review finding 6.
    for cmd in [
        "docker push img:1",
        "podman push img:1",
        "docker image push img:1",
        "npm publish",
        "pnpm publish --access public",
        "yarn publish",
        "cargo publish -p ways",
        "gh release upload v1 a.tgz",
        "gh release create v1 --notes x",
        "gh secret set TOKEN",
        "gh variable set NAME",
        "gh gist create f.txt",
        "gh api -X POST repos/o/r/issues",
        "gh api -XPOST repos/o/r/issues",
        "gh api --method=DELETE repos/o/r/git/refs/x",
        "gh api --method PATCH repos/o/r",
    ] {
        assert_eq!(class(cmd), Some(Class::Publish), "{cmd}");
    }
    assert_eq!(class("gh api repos/o/r/pulls"), Some(Class::VcsRead));
    assert_eq!(class("gh api -X GET repos/o/r"), Some(Class::VcsRead));
    assert_eq!(class("docker pull img"), Some(Class::Service));
    assert_eq!(class("cargo build"), Some(Class::BuildTest));
}

#[test]
fn git_value_flags_skip_their_values() {
    // Review finding 7.
    assert_eq!(
        sub("git --git-dir /x/.git status").as_deref(),
        Some("status")
    );
    assert_eq!(sub("git --work-tree w log").as_deref(), Some("log"));
    assert_eq!(sub("git --namespace ns push").as_deref(), Some("push"));
    assert_eq!(class("git --git-dir /x/.git push"), Some(Class::Publish));
}

#[test]
fn docker_subcommands_take_a_second_word_only_where_listed() {
    // Review finding 8.
    assert_eq!(
        sub("docker compose -f x.yml up -d").as_deref(),
        Some("compose up")
    );
    assert_eq!(sub("docker image ls").as_deref(), Some("image ls"));
    assert_eq!(sub("docker run -it ubuntu bash").as_deref(), Some("run"));
    assert_eq!(sub("podman build -t x .").as_deref(), Some("build"));
}

#[test]
fn command_v_is_a_lookup() {
    // Review finding 9.
    let segs = parse("command -v cargo && command -V git");
    for s in &segs {
        assert_eq!(s.program.as_deref(), Some("command"));
        assert_eq!(s.class, Some(Class::ReadInspect));
    }
    // `command x` without -v still runs x.
    assert_eq!(parse("command git push")[0].program.as_deref(), Some("git"));
}

#[test]
fn wrapper_flag_cluster_ending_in_a_value_flag() {
    let segs = parse("sudo -iu postgres psql");
    assert_eq!(segs[0].program.as_deref(), Some("psql"));
    // An attached value is not a cluster: `-uroot` takes nothing more.
    assert_eq!(
        parse("sudo -uroot whoami")[0].program.as_deref(),
        Some("whoami")
    );
}

#[test]
fn time_is_a_wrapper_with_flags() {
    let segs = parse("time -p cargo build");
    assert_eq!(
        (segs[0].program.as_deref(), segs[0].class),
        (Some("cargo"), Some(Class::BuildTest))
    );
}

#[test]
fn make_jobs_and_variables_are_not_subcommands() {
    assert_eq!(sub("make -j 4 install").as_deref(), Some("install"));
    assert_eq!(sub("make -j install").as_deref(), Some("install"));
    assert_eq!(sub("make V=1 install").as_deref(), Some("install"));
    assert_eq!(sub("make -j4 test").as_deref(), Some("test"));
}

#[test]
fn python_module_after_interpreter_flags() {
    assert_eq!(sub("python3 -u -m pytest -q").as_deref(), Some("pytest"));
    assert_eq!(sub("python3 script.py -m x"), None);
}

#[test]
fn git_tag_listing_is_a_read() {
    assert_eq!(class("git tag -l 'v1*'"), Some(Class::VcsRead));
    assert_eq!(class("git tag"), Some(Class::VcsRead));
    assert_eq!(class("git tag v1.0"), Some(Class::VcsWrite));
    assert_eq!(class("git tag -d v1.0"), Some(Class::VcsWrite));
}

#[test]
fn kubectl_mutations_are_service() {
    for cmd in [
        "kubectl apply -f x.yml",
        "kubectl delete pod x",
        "kubectl create ns y",
        "kubectl patch deploy z -p '{}'",
    ] {
        assert_eq!(class(cmd), Some(Class::Service), "{cmd}");
    }
    assert_eq!(class("kubectl -n foo get pods"), Some(Class::Other));
}

#[test]
fn find_that_removes_is_a_write() {
    assert_eq!(
        class("find . -name '*.o' -exec rm {} \\;"),
        Some(Class::FileWrite)
    );
    assert_eq!(
        class("find . -name '*.o' -execdir /bin/rm -f {} +"),
        Some(Class::FileWrite)
    );
    assert_eq!(class("find . -name '*.o' -delete"), Some(Class::FileWrite));
    assert_eq!(
        class("find . -name '*.rs' -exec grep -l x {} +"),
        Some(Class::Search)
    );
}

#[test]
fn a_command_string_carries_its_commands_class() {
    let inner = |cmd: &str| {
        parse(cmd)
            .into_iter()
            .filter(|s| s.program.as_deref() == Some("git"))
            .map(|s| (s.role, s.class))
            .collect::<Vec<_>>()
    };
    let publish = vec![(Role::Operative, Some(Class::Publish))];
    assert_eq!(inner("eval \"git push --force\""), publish);
    assert_eq!(inner("eval git push"), publish);
    assert_eq!(inner("bash -c \"git push origin main\""), publish);
    assert_eq!(inner("sudo sh -c 'git push'"), publish);
    assert_eq!(inner("env FOO=1 bash -c \"git push\""), publish);
    assert_eq!(inner("bash -c \"sh -c 'git push'\""), publish);
    assert_eq!(class("bash -lc \"make test\""), Some(Class::Other));
    assert_eq!(
        parse("bash -lc \"make test\"")[1].class,
        Some(Class::BuildTest)
    );
    assert_eq!(parse("eval")[0].role, Role::Navigation);
}
