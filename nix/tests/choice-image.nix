{
  runCommand,
  dosfstools,
  mtools,
  util-linux,
  mode ? "nixos-current",
  entry ? "",
  ...
}:
runCommand "decider-choice-fat16.img"
  {
    nativeBuildInputs = [
      dosfstools
      mtools
      util-linux
    ];
  }
  ''
    set -euo pipefail

    img="$TMPDIR/choice.img"
    offset_sectors=2048
    offset_bytes=$((offset_sectors * 512))

    truncate -s 64M "$img"

    sfdisk "$img" <<'EOF' >/dev/null
    label: dos
    unit: sectors

    start=2048, type=c, bootable
    EOF

    mkfs.vfat -F 16 --offset="$offset_sectors" "$img" >/dev/null

    cat > "$TMPDIR/DECIDER.CHO" <<'EOF'
    mode=${mode}
    entry=${entry}
    EOF

    mcopy -i "$img@@$offset_bytes" "$TMPDIR/DECIDER.CHO" ::DECIDER.CHO
    cp "$img" "$out"
  ''
