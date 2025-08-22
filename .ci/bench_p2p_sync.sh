#!/bin/bash

###########################################################
# Measures a client syncing 1000 blocks from another client
###########################################################

network_id=1
min_height=250

# The number of clients that are syncing
num_clients=1

# Adjust this to show more/less log messages
log_filter="info,snarkos_node_sync=trace,snarkos_node_rest=warn"

max_wait=1800 # Wait for up to 30 minutes
poll_interval=1 # Check block heights every second

. ./.ci/utils.sh

# Running sums for variance: use sum and sumsq for unbiased sample variance
sum_speed=0
sumsq_speed=0
samples=0
max_speed=0

# Fetch sync speeds from clients via REST and accumulate stats
function sample_sync_speeds() {
  for ((client_index = 1; client_index <= num_clients; client_index++)); do
    port=$((3030 + client_index))
    resp=$(curl -s "http://127.0.0.1:$port/$network_name/sync/status" || true)

    # Skip if response missing
    if [[ -z "$resp" ]]; then
      continue
    fi

    speed=$(echo "$resp" | jq -r '.sync_speed_bps')

    # Skip null or empty
    if [[ -z "$speed" ]] || [[ "$speed" == "null" ]]; then
      echo "Invalid speed value $speed"
      continue
    fi

    # Validate numeric (allow exponent)
    if [[ ! "$speed" =~ ^[+-]?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?$ ]]; then
        echo "Invalid speed value $speed"
       continue
    fi

    echo "Sync speed: $speed"
    if (( speed > max_speed )); then
       max_speed=$speed
    fi

    # Convert to fixed decimal for bc -l
    speed_dec=$(awk -v x="$speed" 'BEGIN{printf "%.12f", x}')
    if [[ -z "$speed_dec" ]]; then
      continue
    fi

    # Accumulate using bc -l for floating point
    sum_speed=$(echo "$sum_speed + $speed_dec" | bc -l)
    sumsq_speed=$(echo "$sumsq_speed + ($speed_dec * $speed_dec)" | bc -l)
    samples=$((samples + 1))
  done
}

network_name=$(get_network_name $network_id)
echo "Using network: $network_name (ID: $network_id)"

snapshot_info=$(<info.txt)
echo "Snapshot_info:"
echo "${snapshot_info}"

# Create log directory
log_dir=".logs-$(date +"%Y%m%d%H%M%S")"
mkdir -p "$log_dir"

# Define a trap handler that cleans up all processes on exit.
trap stop_nodes EXIT
trap child_exit_handler CHLD

# Define a trap handler that prints a message when an error occurs 
trap 'echo "⛔️ Error in $BASH_SOURCE at line $LINENO: \"$BASH_COMMAND\" failed (exit $?)"' ERR

# Shared flags betwen all nodes
common_flags=" --nodisplay --network $network_id --nocdn --dev-num-validators=40 \
  --no-dev-txs --log-filter=$log_filter"

# The client that has the ledger
# (runs on the first two cores)
taskset -c 0,1 snarkos start --dev 0 --client ${common_flags} \
  --logfile="$log_dir/client-0.log" &
PIDS[0]=$!

# Spawn the clients that will sync the ledger
# (running on the other two cores)
for ((client_index = 1; client_index <= num_clients; client_index++)); do
  prev_port=$((4130+client_index-1))

  # Ensure there are no old ledger files and the node syncs from scratch
  snarkos clean --dev $client_index --network $network_id || true

  taskset -c 2,3 snarkos start --dev $client_index --client ${common_flags} \
    --logfile="$log_dir/client-$client_index.log" \
    --peers=127.0.0.1:$prev_port &
  PIDS[client_index]=$!

  # Add 1-second delay between starting nodes to avoid hitting rate limits
  sleep 1
done

wait_for_nodes 0 $((num_clients+1))

# Check heights periodically with a timeout
total_wait=0
while (( total_wait < max_wait )); do
  # Sample sync speed(s) for variance calculation
  sample_sync_speeds

  if check_heights 0 $((num_clients+1)) $min_height "$network_name"; then
    # Use floating point division
    throughput=$(bc <<< "scale=2; $min_height/$total_wait")

    # Compute unbiased sample variance of sync_speed_bps (in blocks^2/s^2)
    if (( samples > 1 )); then
      mean_speed=$(echo "scale=8; $sum_speed / $samples" | bc -l)
      variance=$(echo "scale=8; (($sumsq_speed / $samples) - ($mean_speed * $mean_speed)) * ($samples / ($samples - 1))" | bc -l)
    else
      mean_speed=$(echo "scale=8; 0" | bc -l)
      variance=$(echo "scale=8; 0" | bc -l)
    fi

    echo "🎉 Test passed!. Waited $total_wait for $min_height blocks. Throughput was $throughput blocks/s."

    # Append data to results file.
    printf "{ \"name\": \"p2p-sync\", \"unit\": \"blocks/s\", \"value\": %.3f, \"extra\": \"total_wait=%is, target_height=%i, %s\" },\n" \
       "$throughput" "$total_wait" "$min_height" "$snapshot_info" | tee -a results.json
    printf "{ \"name\": \"p2p-sync-speed-variance\", \"unit\": \"blocks^2/s^2\", \"value\": %.6f, \"extra\": \"samples=%d, mean_bps=%.6f, max_speed=%d, %s\" },\n" \
       "$variance" "$samples" "$mean_speed" "$max_speed" "$snapshot_info" | tee -a results.json
    exit 0
  fi
  
  # Continue waiting
  sleep $poll_interval
  total_wait=$((total_wait+poll_interval))

  echo "Waited $total_wait seconds so far..."
done

echo "❌ Test failed! Clients did not sync within 30 minutes."

# Print logs for debugging
echo "Last 20 lines of client logs:"
for ((client_index = 0; client_index < num_clients; client_index++)); do
  echo "=== Client $client_index logs ==="
  tail -n 20 "$log_dir/client-$client_index.log"
done

exit 1
