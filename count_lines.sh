#/bin/bash
tree -Q -f --gitignore | grep src | while read -r l; do export f="$(awk -F'"' '{ print $2 }' <<< "$l")"; [ -f "$f" ] && echo "$f $(printf "%5d" "$(wc -l < "$f")")" || echo "$f -----" ; done | column -t | grep -P "[0-9]" | sort -k2 -n | cat <<< "" | cat <<< "SUM $(tree -Q -f --gitignore | grep src | while read -r l; do export f="$(awk -F'"' '{ print $2 }' <<< "$l")"; [ -f "$f" ] && echo "$f $(printf "%5d" "$(wc -l < "$f")")" || echo "$f -----" ; done | column -t | grep -P "[0-9]" | sort -k2 -n | awk '{ print $2 }' | paste -sd+  | bc)" | column -t
