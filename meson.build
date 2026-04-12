project('rudo-c', 'c', version : '0.1.0', meson_version : '>=0.60.0', default_options : [
  'c_std=c11',
  'warning_level=2',
  'buildtype=debugoptimized',
])

cc = meson.get_compiler('c')

add_project_arguments([
  '-D_POSIX_C_SOURCE=200809L',
  '-D_XOPEN_SOURCE=700',
], language : 'c')

inc = include_directories('include')
protocol_inc = include_directories('protocols')
root_inc = include_directories('.')

dl_dep = cc.find_library('dl', required : false)
m_dep = cc.find_library('m', required : false)
wayland_client_dep = dependency('wayland-client', required : false)
xkbcommon_dep = dependency('xkbcommon', required : false)
wayland_scanner = find_program('wayland-scanner', required : false)
have_wayland = wayland_client_dep.found() and xkbcommon_dep.found() and wayland_scanner.found()

rudo_public_headers = files(
  'include/rudo/wayland.h',
  'include/rudo/common.h',
  'include/rudo/defaults.h',
  'include/rudo/log.h',
  'include/rudo/input.h',
  'include/rudo/keybindings.h',
  'include/rudo/toml.h',
  'include/rudo/config.h',
  'include/rudo/cli.h',
  'include/rudo/core.h',
  'include/rudo/render.h',
  'include/rudo/terminal.h',
)

rudo_core_sources = files(
  'src/util.c',
  'src/log.c',
  'src/input.c',
  'src/keybindings.c',
  'src/toml.c',
  'src/config.c',
  'src/cli.c',
  'src/render_font.c',
  'src/cursor.c',
  'src/cursor_vfx_full.c',
  'src/software_renderer.c',
  'src/terminal.c',
  'src/pty.c',
  'src/core.c',
)

rudo_wayland_sources = files(
  'src/wayland.c',
  'src/wayland_keyboard.c',
)

rudo_sources = rudo_core_sources
rudo_deps = [dl_dep, m_dep]
if have_wayland
  protocol_headers = [
    custom_target(
      'xdg-shell-client-header',
      input : 'protocols/xdg-shell.xml',
      output : 'xdg-shell-client-protocol.h',
      command : [wayland_scanner, 'client-header', '@INPUT@', '@OUTPUT@'],
    ),
    custom_target(
      'xdg-decoration-client-header',
      input : 'protocols/xdg-decoration-unstable-v1.xml',
      output : 'xdg-decoration-unstable-v1-client-protocol.h',
      command : [wayland_scanner, 'client-header', '@INPUT@', '@OUTPUT@'],
    ),
    custom_target(
      'viewporter-client-header',
      input : 'protocols/viewporter.xml',
      output : 'viewporter-client-protocol.h',
      command : [wayland_scanner, 'client-header', '@INPUT@', '@OUTPUT@'],
    ),
    custom_target(
      'fractional-scale-client-header',
      input : 'protocols/fractional-scale-v1.xml',
      output : 'fractional-scale-v1-client-protocol.h',
      command : [wayland_scanner, 'client-header', '@INPUT@', '@OUTPUT@'],
    ),
  ]
  protocol_sources = [
    custom_target(
      'xdg-shell-private-code',
      input : 'protocols/xdg-shell.xml',
      output : 'xdg-shell-protocol.c',
      command : [wayland_scanner, 'private-code', '@INPUT@', '@OUTPUT@'],
    ),
    custom_target(
      'xdg-decoration-private-code',
      input : 'protocols/xdg-decoration-unstable-v1.xml',
      output : 'xdg-decoration-unstable-v1-protocol.c',
      command : [wayland_scanner, 'private-code', '@INPUT@', '@OUTPUT@'],
    ),
    custom_target(
      'viewporter-private-code',
      input : 'protocols/viewporter.xml',
      output : 'viewporter-protocol.c',
      command : [wayland_scanner, 'private-code', '@INPUT@', '@OUTPUT@'],
    ),
    custom_target(
      'fractional-scale-private-code',
      input : 'protocols/fractional-scale-v1.xml',
      output : 'fractional-scale-v1-protocol.c',
      command : [wayland_scanner, 'private-code', '@INPUT@', '@OUTPUT@'],
    ),
  ]
  rudo_sources += rudo_wayland_sources + protocol_sources + protocol_headers
  rudo_deps += [wayland_client_dep, xkbcommon_dep]
endif

rudo_core = static_library(
  'rudo_core',
  rudo_sources,
  include_directories : [inc, root_inc, protocol_inc],
  dependencies : rudo_deps,
  install : false,
)

rudo_dep = declare_dependency(
  include_directories : [inc, root_inc, protocol_inc],
  dependencies : rudo_deps,
  link_with : rudo_core,
)

install_headers(rudo_public_headers, subdir : 'rudo')

pkg = import('pkgconfig')
pkg.generate(
  name : 'rudo-c',
  filebase : 'rudo-c',
  description : 'rudo C core scaffolding',
  version : meson.project_version(),
  libraries : rudo_core,
  subdirs : 'rudo',
)

if have_wayland
  rudo_exe = executable(
    'rudo',
    ['src/main.c'],
    include_directories : [inc, root_inc, protocol_inc],
    link_with : rudo_core,
    dependencies : [wayland_client_dep, xkbcommon_dep, dl_dep, m_dep],
    install : true,
  )
endif


test_deps = [rudo_dep]

test_terminal = executable(
  'test_terminal',
  'tests/test_terminal.c',
  include_directories : [inc, root_inc, protocol_inc],
  dependencies : test_deps,
  install : false,
)

test_core = executable(
  'test_core',
  'tests/test_core.c',
  include_directories : [inc, root_inc, protocol_inc],
  dependencies : test_deps,
  install : false,
)

test('terminal', test_terminal)
test('core', test_core)
