# termvide terminfo

Termvide ships a bundled `termvide` terminfo source at `extra/terminfo/termvide.info`.

The entry intentionally stays close to `xterm-256color` and only carries a small set of
extra or reaffirmed capabilities that match current practical behavior in Termvide.
That keeps `TERM=termvide` useful without advertising terminal features that are not yet a
stable part of the emulator contract.

## What the bundled entry currently assumes

`termvide` still inherits almost everything from `xterm-256color`, including:

- 256-color support
- direct color via `Tc`
- alternate screen, cursor addressing, and the usual xterm-style key definitions
- DEC special graphics / ACS support through inherited `acsc`, `smacs`, and `rmacs`
- `kmous` and the rest of the standard xterm mouse/key baseline

On top of that, the bundled entry keeps a few practical extended capabilities that Termvide
currently supports well:

- OSC 52 clipboard set via `Ms`
- cursor style control via `Ss` / `Se`
- focus in/out reporting via `fe` / `fd`
- bracketed paste via `BE` / `BD`
- SGR mouse enable/disable via `XM`, with SGR mouse event format in `xm`

## Why the entry is intentionally conservative

Recent terminal-side changes make a conservative entry more accurate than a heavily customized
one.

In particular, the bundled source does **not** try to claim support for things that are not
clearly represented as stable terminfo capabilities today, such as:

- kitty keyboard protocol / `CSI u`
- modifyOtherKeys-style extended keyboard reporting
- synchronized updates as a terminfo capability
- any richer clipboard contract beyond OSC 52 set

Those may exist in parts of the codebase or be queryable in other ways, but the bundled
terminfo should describe what applications can safely rely on through terminfo today.

## Install

```sh
./extra/terminfo/install-termvide-terminfo.sh
```

This compiles the entry with `tic -x` into `~/.terminfo` by default.
Override the install destination with `TERMINFO` or `TERMINFO_DIRS_OVERRIDE` if needed.

## TERM selection behavior

Termvide chooses `TERM` for spawned shells in this order:

1. If `TERMVIDE_TERM` is set to a non-empty value, that exact value wins.
2. Otherwise, if a compiled `termvide` terminfo entry appears to be installed locally,
   Termvide uses `TERM=termvide`.
3. Otherwise, Termvide falls back to `TERM=xterm-256color`.

The availability check is intentionally lightweight: it looks for a compiled `termvide`
entry in the usual terminfo locations, honoring `TERMINFO`, `TERMINFO_DIRS`, `~/.terminfo`,
and standard system directories such as `/usr/share/terminfo`.
It checks both common on-disk layouts used by terminfo databases:

- `t/termvide`
- `74/termvide`

That keeps the default safe: Termvide only advertises `TERM=termvide` when the entry looks
present already, avoiding a TERM/terminfo mismatch for child applications.

## Override examples

Force the bundled entry:

```sh
TERMVIDE_TERM=termvide termvide
```

Force the conservative fallback:

```sh
TERMVIDE_TERM=xterm-256color termvide
```

Or point `TERMVIDE_TERM` at any other installed TERM value.

## Verify

Inside a shell spawned by Termvide:

```sh
echo "$TERM"
infocmp -x termvide
```

## Notes for users opting into `TERM=termvide`

Practical caveats:

- The entry is xterm-derived, so applications will mostly behave as if they were running in
  `xterm-256color`, with a few extra capabilities exposed explicitly.
- Mouse reporting is described in the usual xterm way: applications still need to enable the
  appropriate mouse mode themselves, and SGR mouse is only meaningful when those modes are on.
- OSC 52 in the bundled entry is for clipboard **set** behavior. Applications should not assume a
  broader clipboard query/round-trip contract from terminfo alone.
- ACS support is inherited from `xterm-256color`; the bundled entry does not need to restate it.
- If an application depends on bleeding-edge keyboard protocol features rather than classic
  xterm-compatible keys, `TERM=xterm-256color` may still be the more compatible choice until
  `termvide` grows a clearer long-term keyboard contract.
