# Security policy

Rexon installs and removes system software through privileged helpers
(see [What Rexon does with root](README.md#what-rexon-does-with-root)), so security
reports are taken seriously.

## Reporting a vulnerability

Please report vulnerabilities **privately** through GitHub:
open the repository's **Security** tab and choose **Report a vulnerability**
(GitHub Security Advisories). Do not open a public issue or pull request for a
security problem.

Useful to include:

- what an attacker can do, and from which position (another local user, a
  program running as your user, a malicious theme or package from a store, the
  network);
- the affected component (for example `rexon-priv-helper`, `rexon-pkg-helper`, a
  polkit policy, the `.rxn` installer) and the Rexon version or commit;
- steps or a proof of concept.

You will get an answer as soon as possible. Once a fix is released, the advisory
is published with credit to the reporter, unless you prefer not to be named.

## Scope

Especially interesting:

- getting root through the helpers or the polkit actions `org.rexon.*` without
  the administrator password, or after the right should have ended (window
  closed, 15 minutes idle, screen locked, sleep);
- making a helper install a package outside Rexon's tables, remove a protected
  package (kernel, drivers, boot loader, the current desktop), or write outside its
  fixed destinations;
- code from a theme, wallpaper or `.rxn` package that runs as root, or runs as the
  user without being a `desktop-tool` / `plugin` extension the user chose to
  install.

Vulnerabilities in third-party components (waywallen, Open Wallpaper Engine, Qt,
desktop environments) should be reported to their projects; if Rexon makes one
easier to exploit, please tell us too.

## Supported versions

Rexon has no stable releases yet. Fixes go to the main branch and the next package
build.
