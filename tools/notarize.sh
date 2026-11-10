#!/bin/sh
# Notarizes FILE with Apple and staples the ticket to TARGET, so Gatekeeper
# accepts it without asking, offline too.
#
#   NOTARY_KEY=AuthKey.p8 NOTARY_KEY_ID=… NOTARY_ISSUER=… tools/notarize.sh FILE [TARGET]
#
# FILE is what is uploaded: a .zip of the app (TARGET is then the app), or a
# signed .dmg (TARGET defaults to FILE). The key is an App Store Connect API
# key with the Developer role. On rejection, prints Apple's log and fails.
set -eu

file=$1
target=${2:-$1}
auth() {
    "$@" --key "$NOTARY_KEY" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER"
}

result=$(auth xcrun notarytool submit "$file" --wait --timeout 2h --output-format json)
echo "$result"
id=$(printf '%s' "$result" | plutil -extract id raw -o - -)
status=$(printf '%s' "$result" | plutil -extract status raw -o - -)
if [ "$status" != Accepted ]; then
    auth xcrun notarytool log "$id"
    echo "notarization of $file: $status" >&2
    exit 1
fi
xcrun stapler staple "$target"
xcrun stapler validate "$target"
