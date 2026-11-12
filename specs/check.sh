#!/bin/sh
set -eu
jar=${TLA_JAR:-/tla/tla2tools.jar}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM
cp -R "$(dirname "$0")" "$work/specs"
cd "$work/specs"
java -Xmx2g -XX:+UseParallelGC -jar "$jar" -workers 2 -cleanup -config cfg/provider-protocol.cfg ProviderProtocol.tla > "$work/base.log" 2>&1 || {
    cat "$work/base.log"; exit 1;
}
grep -Fq 'Model checking completed. No error' "$work/base.log" || { cat "$work/base.log"; exit 1; }
cat "$work/base.log"
for scenario in provider-protocol-mutant:CorrelationSafety provider-cancel-mutant:CancelAdvisory provider-partial-mutant:PartialOptIn provider-reader-mutant:StaleReaderIsolation; do
    config=${scenario%:*}
    invariant=${scenario#*:}
    status=0
    java -Xmx2g -XX:+UseParallelGC -jar "$jar" -workers 2 -cleanup -config "cfg/$config.cfg" ProviderProtocol_Mutant.tla > "$work/$config.log" 2>&1 || status=$?
    # TLC exit 12 means a safety invariant counterexample. Parser errors,
    # runtime exceptions and a passing mutant must all fail the gate.
    if [ "$status" -ne 12 ] || ! grep -Fq "Invariant $invariant is violated" "$work/$config.log"; then
        cat "$work/$config.log"; exit 1
    fi
    printf '%s: killed by %s\n' "$config" "$invariant"
done

java -Xmx2g -XX:+UseParallelGC -jar "$jar" -workers 2 -cleanup -config cfg/commit-history.cfg CommitHistory.tla > "$work/history.log" 2>&1 || {
    cat "$work/history.log"; exit 1;
}
grep -Fq 'Model checking completed. No error' "$work/history.log" || { cat "$work/history.log"; exit 1; }
cat "$work/history.log"
for scenario in stale:FreshLanding unbounded:BoundedRetention after-terminal:UniqueTerminal forget-clipping:TruthfulCompletion erase-partials:FailurePreservesPartials; do
    fault=${scenario%:*}
    invariant=${scenario#*:}
    status=0
    java -Xmx2g -XX:+UseParallelGC -jar "$jar" -workers 2 -cleanup -config "cfg/commit-history-$fault.cfg" CommitHistory.tla > "$work/history-$fault.log" 2>&1 || status=$?
    if [ "$status" -ne 12 ] || ! grep -Fq "Invariant $invariant is violated" "$work/history-$fault.log"; then
        cat "$work/history-$fault.log"; exit 1
    fi
    printf 'commit-history-%s: killed by %s\n' "$fault" "$invariant"
done
