# dmgbuild settings for the Knack installer window.
# Usage (from the repo root): dmgbuild -s assets/dmg/dmg-settings.py -D app=path/to/Knack.app Knack out.dmg
import os.path

application = defines.get("app", "target/release/Knack.app")  # noqa: F821
app_name = os.path.basename(application)

format = "UDZO"
files = [application]
symlinks = {"Applications": "/Applications"}
hide_extension = [app_name]
icon = os.path.join(application, "Contents/Resources/AppIcon.icns")  # volume icon

background = defines.get("background", "assets/dmg/background.png")  # noqa: F821  (@2x picked up for Retina)
window_rect = ((200, 160), (640, 392))  # ~360pt of content below the title bar
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
show_icon_preview = False

icon_size = 112
text_size = 13
label_pos = "bottom"
arrange_by = None
# Keep in sync with render-background.swift.
icon_locations = {app_name: (180, 160), "Applications": (460, 160)}
