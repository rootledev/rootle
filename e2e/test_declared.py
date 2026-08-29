"""Declared-provider lifecycle e2e (plans/0019 M2): a config naming a
provider this machine doesn't have triggers the consent popup at
startup — the trust line names the source, declining lands in honest
degraded mode with the declaration still in the config. Offline
throughout: accepting is never exercised here (it would hit the
network); the verified install flow itself is covered by the manager's
loopback tests."""

from pathlib import Path

from tui import Tui, build


def test_declared_missing_asks_then_degrades_honestly(tmp_path: Path) -> None:
    config = tmp_path / "declared.toml"
    config.write_text('[provider]\nkind = "gitlab"\n')
    tui = Tui(build(), cols=170, rows=30, args=["--config", str(config)]).start()
    try:
        screen = tui.expect("install provider?")
        assert "config declares gitlab" in screen, screen
        assert "you are trusting rootledev/rootle-gitlab" in screen, screen
        assert "y install" in screen, screen

        tui.send("n")
        screen = tui.expect("browsing github")
        assert "gitlab not installed" in screen, screen
        assert "retry: rootle provider install gitlab" in screen, screen

        # The declaration stays in the config, untouched.
        assert 'kind = "gitlab"' in config.read_text()
    finally:
        tui.stop()


def test_declared_pin_fields_surface_in_the_popup(tmp_path: Path) -> None:
    config = tmp_path / "pinned.toml"
    config.write_text(
        '[provider]\nkind = "bitbucket"\ntag = "v0.1.4"\n'
        'sha = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"\n'
    )
    tui = Tui(build(), cols=170, rows=30, args=["--config", str(config)]).start()
    try:
        screen = tui.expect("install provider?")
        assert "config declares bitbucket" in screen, screen
        assert "rootledev/rootle-bitbucket" in screen, screen
        assert "tag v0.1.4" in screen, screen
        assert "sha256" in screen, screen
        tui.send("n")
        tui.expect("browsing github")
    finally:
        tui.stop()


def test_broken_stdio_raises_health_prompt(tmp_path: Path) -> None:
    """0022 M2: a spawn that fails at startup raises the health prompt
    (r/g/e), and `g` degrades to github with the notice sticky — the
    0018-class race that used to erase it can't happen anymore."""
    config = tmp_path / "broken.toml"
    config.write_text(
        '[provider]\nkind = "stdio"\ncommand = ["/nonexistent/no-such-provider"]\n'
    )
    tui = Tui(build(), cols=130, rows=30, args=["--config", str(config)]).start()
    try:
        screen = tui.expect("provider health")
        assert "stdio failed to start" in screen or "failed to start" in screen, screen
        assert "r retry" in screen, screen

        # g — browse github, and the notice stays on the modeline
        # (middle-truncated — assert the surviving fragments).
        tui.send("g")
        screen = tui.expect("not installed")
        assert "provider install" in screen, screen
    finally:
        tui.stop()


def test_tarball_kind_health_no_retry(tmp_path: Path) -> None:
    """A kind naming a plain-HTTP tarball is never auto-fetched (0019
    rule) and never retryable: the health prompt offers g/e only."""
    config = tmp_path / "tarball.toml"
    config.write_text(
        '[provider]\nkind = "https://artifacts.corp.example/rootle-x.tar.gz"\n'
    )
    tui = Tui(build(), cols=130, rows=30, args=["--config", str(config)]).start()
    try:
        screen = tui.expect("provider health")
        assert "g browse github" in screen, screen
        assert "r retry" not in screen, screen
    finally:
        tui.stop()
