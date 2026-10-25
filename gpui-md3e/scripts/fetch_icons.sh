#!/usr/bin/env bash
# Fetches the Material Symbols (Rounded, Apache-2.0) subset bundled with gpui-md3e.
# Each icon is stored as `<name>.svg` (outlined) and `<name>-fill.svg` (filled).
set -euo pipefail
cd "$(dirname "$0")/../assets/icons"
base=https://raw.githubusercontent.com/google/material-design-icons/master/symbols/web
icons=(
  add add_circle arrow_back arrow_drop_down arrow_forward attach_file auto_awesome
  bookmark call celebration chat chat_bubble check check_circle chevron_left
  chevron_right close content_copy content_paste dark_mode delete done download edit
  add_reaction error expand_less expand_more favorite filter_list folder format_align_center
  format_align_left format_align_right format_bold format_italic format_underlined forum
  group home image inbox info keyboard_arrow_down keyboard_arrow_up label language
  light_mode link location_on lock mail menu mic mood more_horiz more_vert notifications
  palette pause person photo_camera play_arrow public refresh remove rocket_launch schedule
  search send settings share skip_next smart_toy sort star thumb_up translate tune upload
  videocam visibility volume_up warning waving_hand
)
for name in "${icons[@]}"; do
  curl -fsSL -o "$name.svg" "$base/$name/materialsymbolsrounded/${name}_24px.svg"
  curl -fsSL -o "$name-fill.svg" "$base/$name/materialsymbolsrounded/${name}_fill1_24px.svg"
done
