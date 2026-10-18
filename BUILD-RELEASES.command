#!/bin/bash
# Двойной щелчок в Finder: собирает Mac (universal) и Windows x64 версии ORVEXA
# с текущим интерфейсом и оставляет в release/ только по одной свежей копии.
# Старые копии перемещаются в Корзину, а не удаляются навсегда.
set -uo pipefail
cd "$(dirname "$0")"
export ORVEXA_VERSION="${ORVEXA_VERSION:-0.1.0-desktop-preview.4}"
mkdir -p release demo-data
log="$PWD/demo-data/build-releases-$(date +%Y%m%d-%H%M%S).log"
exec > >(tee -a "$log") 2>&1
echo "ORVEXA $ORVEXA_VERSION — сборка релизов. Журнал: $log"

existing() { find release -mindepth 1 -maxdepth 1 -type d -name "$1*" | sort; }
before_mac="$(existing macos-)"; before_win="$(existing windows-; existing desktop-)"

trash() {
  for dir in "$@"; do
    [[ -n "$dir" && -d "$dir" ]] || continue
    target="$HOME/.Trash/$(basename "$dir")-$RANDOM"
    mv "$dir" "$target" && echo "  в Корзину: $dir"
  done
}
fresh() { comm -13 <(printf '%s\n' "$1" | sed '/^$/d' | sort) <(printf '%s\n' "$2" | sed '/^$/d' | sort); }

echo; echo '== 1/2 macOS universal'
if bash scripts/package-macos.sh universal; then mac=ok; else mac=failed; fi
new_mac="$(fresh "$before_mac" "$(existing macos-)")"
if [[ $mac == ok ]]; then
  echo 'macOS готово. Убираю старые копии Mac:'
  # shellcheck disable=SC2086
  trash $before_mac
else
  echo 'macOS: сборка не удалась, старая копия оставлена.'
  # shellcheck disable=SC2086
  trash $new_mac
fi

echo; echo '== 2/2 Windows x64 (кросс-сборка)'
if bash scripts/package-windows-cross.sh; then win=ok; else win=failed; fi
new_win="$(fresh "$before_win" "$(existing windows-; existing desktop-)")"
if [[ $win == ok ]]; then
  echo 'Windows готово. Убираю старые копии Windows:'
  # shellcheck disable=SC2086
  trash $before_win
else
  echo 'Windows: сборка не удалась, старая копия оставлена.'
  # shellcheck disable=SC2086
  trash $new_win
fi
# Remove the empty preview.2 shell and similar leftovers without an app inside.
for dir in $(existing macos-); do [[ -d "$dir/ORVEXA.app/Contents/MacOS" ]] || trash "$dir"; done

echo
echo "Итог: macOS — $mac, Windows — $win"
ls -1 release
echo "Журнал: $log"
[[ $mac == ok && $win == ok ]]
