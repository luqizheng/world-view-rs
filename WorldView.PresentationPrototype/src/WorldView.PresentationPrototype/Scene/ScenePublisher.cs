using System;
using System.Threading;
using WorldView.PresentationPrototype.Diagnostics;

namespace WorldView.PresentationPrototype.Scene;

/// <summary>
/// Atomic publisher for immutable scene snapshots (Handoff §5.4).
///
/// Publication contract:
///  - The reference swap is a single atomic operation; readers observe either the complete
///    previous snapshot or the complete new snapshot, never partial data.
///  - Versions are strictly monotonic: a snapshot with version &lt;= the currently published
///    version is DISCARDED (stale build result), never published.
///  - Cancellation must never publish partial data: builders only call TryPublish after
///    constructing the complete snapshot; this class performs no partial assembly.
///  - A draw already in progress keeps its captured snapshot; the next draw sees the new one.
///    (Enforced by the draw operation holding exactly one snapshot reference.)
/// </summary>
public sealed class ScenePublisher
{
    private SceneSnapshot? _current;

    /// <summary>Raised after a successful publish. Handlers must be fast and must not throw;
    /// UI marshalling is the subscriber's job (Dispatcher.UIThread.Post).</summary>
    public event Action<SceneSnapshot>? Published;

    /// <summary>Currently published snapshot (or null before the first publish).
    /// Atomic single-reference read, safe from any thread.</summary>
    public SceneSnapshot? Current => Volatile.Read(ref _current);

    /// <summary>
    /// Attempt to publish a complete snapshot. Returns true when the snapshot became current;
    /// false when it was discarded as stale (its version was not newer than the published one).
    /// </summary>
    public bool TryPublish(SceneSnapshot snapshot)
    {
        ArgumentNullException.ThrowIfNull(snapshot);

        while (true)
        {
            var current = Volatile.Read(ref _current);

            // Stale-result discard: versions must be strictly monotonic.
            if (current is not null && snapshot.Version <= current.Version)
                return false;

            // Compare-and-swap the single reference; a racing newer publish wins.
            var existing = Interlocked.CompareExchange(ref _current, snapshot, current);
            if (!ReferenceEquals(existing, current))
                continue; // lost a race - retry against the new current

            RenderDiagnostics.Instance.RecordPublish(snapshot.Version);
            RaisePublishedSafely(snapshot);
            return true;
        }
    }

    private void RaisePublishedSafely(SceneSnapshot snapshot)
    {
        var handler = Published;
        if (handler is null) return;

        // Delegate list snapshot; invoke outside any lock so subscribers cannot deadlock
        // publication. Individual subscriber exceptions are isolated.
        foreach (var d in handler.GetInvocationList())
        {
            try
            {
                ((Action<SceneSnapshot>)d)(snapshot);
            }
            catch (Exception ex)
            {
                RenderDiagnostics.Instance.RecordVersionConsistencyViolation(
                    $"Published subscriber threw: {ex.GetType().Name}: {ex.Message}");
            }
        }
    }
}
