#!/usr/bin/env bash
# Verify a newly installed ThinkPad battery is accepted by the EC and really charging.
# Run on AC power, with the battery below ~90% so there's headroom to observe a rise.
# Usage: ./check-new-battery.sh

B=/sys/class/power_supply/BAT0
fail=0

echo "=== IDENTITY ==="
printf "  manufacturer : %s\n" "$(cat $B/manufacturer 2>/dev/null)"
printf "  model_name   : %s\n" "$(cat $B/model_name 2>/dev/null)"
printf "  technology   : %s\n" "$(cat $B/technology 2>/dev/null)"
printf "  serial       : %s\n" "$(cat $B/serial_number 2>/dev/null)"

echo
echo "=== CAPACITY (a healthy new 57Wh pack should read ~55-57 Wh) ==="
full=$(cat $B/energy_full 2>/dev/null)
design=$(cat $B/energy_full_design 2>/dev/null)
awk -v f="$full" -v d="$design" 'BEGIN{
  printf "  energy_full        : %.2f Wh\n", f/1e6
  printf "  energy_full_design : %.2f Wh\n", d/1e6
  printf "  health             : %.1f%%\n", 100*f/d
  if (100*f/d < 90) print "  ** LOW for a new pack - suspect old stock or a relabelled cell"
}'

echo
echo "=== AC + CHARGE STATE ==="
ac=$(cat /sys/class/power_supply/AC/online 2>/dev/null)
status=$(cat $B/status 2>/dev/null)
echo "  AC online : $ac"
echo "  status    : $status"
if [ "$ac" != "1" ]; then
  echo "  ** Plug in AC before running this test."
  exit 1
fi
if [ "$status" != "Charging" ] && [ "$status" != "Full" ]; then
  echo "  ** NOT CHARGING on AC - this is the authentication failure signature."
  fail=1
fi

echo
echo "=== REAL CHARGE TEST (energy must actually rise over 3 min) ==="
e1=$(cat $B/energy_now); echo "  t=0s   $(awk -v e=$e1 'BEGIN{printf "%.3f Wh", e/1e6}')"
sleep 90
e2=$(cat $B/energy_now); echo "  t=90s  $(awk -v e=$e2 'BEGIN{printf "%.3f Wh", e/1e6}')"
sleep 90
e3=$(cat $B/energy_now); echo "  t=180s $(awk -v e=$e3 'BEGIN{printf "%.3f Wh", e/1e6}')"

if [ "$e3" -gt "$e1" ]; then
  awk -v a=$e1 -v b=$e3 'BEGIN{printf "  OK - gained %.3f Wh in 3 min (~%.1f W)\n", (b-a)/1e6, (b-a)/1e6*20}'
else
  echo "  ** NO ENERGY GAIN - the pack reports Charging but is not taking charge. Return it."
  fail=1
fi

echo
echo "=== EC / KERNEL COMPLAINTS ==="
hits=$(journalctl -b -p warning --no-pager 2>/dev/null | grep -icE "battery|acpi.*bat|charg")
echo "  warning-level battery lines this boot: $hits"
[ "$hits" -gt 0 ] && journalctl -b -p warning --no-pager 2>/dev/null | grep -iE "battery|acpi.*bat|charg" | tail -5

echo
if [ "$fail" -eq 0 ]; then
  echo "VERDICT: battery accepted and charging normally."
  echo "Next: set the charge thresholds so this one lasts."
else
  echo "VERDICT: FAILED - start the return while the window is open."
fi
exit $fail
