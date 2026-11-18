# 0073. The oracle joins a stand-in player to record the decoration order

- Status: Accepted
- Date: 2026-11-14
- See also: [0015](0015-locate-checks-on-a-real-server.md), [0053](0053-the-oracle-decorates-chunks-on-their-own.md), [0056](0056-two-certainties-on-its-own-and-whatever-the-neighbours.md)

## Context

[ADR 0056](0056-two-certainties-on-its-own-and-whatever-the-neighbours.md) treats the order chunks are decorated in as unknown. To find out whether it can be known around the spawn, the oracle has to record the order the game itself uses there.

The game decorates little before a player arrives. In 26.2 to 26.4, a new world's startup asks only for the chunks its spawn search visits, which decorates the 3 × 3 around the spawn origin chunk and around each chunk of the search, about 48 blocks across ([research](../research/decoration-order.md#what-the-game-decorates-at-creation)). The area that spawn searches for dungeons and loot ask about is decorated when the first player joins, through the configuration phase's spawn task and the player's view-distance tickets. The order there depends on those tickets and on the worker threads' timing.

The `locate` server ([ADR 0015](0015-locate-checks-on-a-real-server.md)) runs the stock server with no player, and nothing in the game holds the running server where a harness could reach it.

## Decision

We will record the join in the harness's own JVM. `spyglass.oracle.Join` will:

- start the stock `net.minecraft.server.Main` with `--jfrProfile`, so the game's JFR events are recorded from before the world loads;
- find the `MinecraftServer` through the server thread's task, the `AtomicReference` that `MinecraftServer.spin` gives it, by reflection (`--add-opens java.base/java.lang=ALL-UNNAMED`);
- join one player as a client's join would: the configuration phase's `PrepareSpawnTask` started, ticked until it's ready, then its `spawnPlayer`, which places the player with `PlayerList.placeNewPlayer`. The only stand-in is the connection, a `Connection` on Netty's `EmbeddedChannel`, as the game's own gametest mock player has;
- wait until every chunk within the view distance is loaded in full, wait a little longer, and stop the server, which writes the recording to `debug/`.

The xtask reads the recording's `minecraft.ChunkGeneration` events with the JDK's `jfr` tool.

## Consequences

- The tickets, the spawn search, the scheduler and every generation step are the game's. Only the network connection and the way the server is found are the harness's.
- 26.2, 26.3 and 26.4 share every method it calls, so it lives in the oracle's core with no adapter code. A version that changes them gets adapter methods.
- Reading `Thread`'s private fields depends on the JDK's internals. If they change, the run fails at once, saying it found no server.
- No client answers the server's keep-alives or acknowledges chunk batches. Generation doesn't wait on either, but the server drops an unresponsive player after a time, so loading has to finish before then. A run where the player is removed first fails rather than recording a partial area.
- The spawn task picks a different position in the spawn radius on each run, as it does for a real first join, so the recorded area moves between runs. That's part of what's measured.
- A run takes about a minute on 26.3 (seed 1, view distance 10: about 13 s to load the view distance after the join), so like `locate` it's not part of `oracle golden` or CI.

## Alternatives considered

- **A headless client over TCP**, logging in to the stock server: nothing in the server changes, but the protocol changes between versions, and the login, configuration and play phases are a lot of code to keep up.
- **Startup only:** no player needed, but it covers about 48 blocks, far less than the area spawn searches ask about.
- **`/forceload` from the console:** no new code, but forced tickets aren't a player's, so the order recorded isn't a join's.
- **Building the server in the harness**, by repeating `Main`'s setup: it reaches the server without reflection, but copies the code under test, which [ADR 0015](0015-locate-checks-on-a-real-server.md) rejected for locate.
