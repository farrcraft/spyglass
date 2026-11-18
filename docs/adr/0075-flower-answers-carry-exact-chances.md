# 0075. Flower answers carry exact chances

- Status: Accepted
- Date: 2026-11-15
- See also: [0056](0056-two-certainties-on-its-own-and-whatever-the-neighbours.md)

## Context

spyglass's answers so far are certain or candidates. A structure starts or may start; a feature places or may place, on its own or whatever the neighbours do ([ADR 0056](0056-two-certainties-on-its-own-and-whatever-the-neighbours.md)). A candidate is something spyglass can't decide: a tie, a feature it doesn't place, an order nobody knows.

The flower bone meal grows on grass at a block ([flowers.md](../specs/flowers.md)) is often decided by the player's own random draws, not by anything spyglass doesn't know:

- A meadow or birch forest picks one of two bone meal features at random.
- A `weighted` provider (the default flowers, cherry petals, wildflowers) picks by weight.
- Plains' `noise_threshold` provider uses the noise only to split tulips from the other flowers, then picks within each part at random: 1 in 4 for each tulip, or 1 in 12 for each high flower against 2 in 3 for a dandelion.

Calling all of these candidates would lose what a farm builder needs. A plains block that is 25% red tulip isn't the same answer as one that is 8% red tulip and 67% dandelion, and every plains block would be a candidate for something. The chances are fixed by the game's code and data, so spyglass can compute them exactly.

## Decision

We will give flower answers as each flower's exact chance at the block, given that bone meal makes a flower there, as a fraction (`spyglass::Chance`, a numerator and denominator). A flower is certain where its chance is 1. A tied biome cell gives one set of chances per candidate biome, as other answers give candidates.

- The chances come from the code's structure, not from sampling: `1/n` per bone meal feature, `weight / total` for weighted lists, `1/|list|` for `Util.getRandom`, and for a `nextFloat() < p` test the share of `nextFloat()`'s 2²⁴ values below `p`.
- They're the chance at a block, not per click: where bone meal's walk lands, and whether that try makes a flower at all (1 in 8), are left out, since they don't depend on the block.
- A search can ask for a field where a flower's chance is at least some share, 100% (certain) by default.

## Consequences

- A farm builder sees what a block will grow and how often, and a search can ask for "a field where cornflowers are certain" or "where red tulips are at least 25%".
- Chances are exact, so a block's chances sum to exactly 1, and two answers compare without rounding. The oracle checks the draws they come from, state by state. The chances themselves are checked against draws statistically, by unit tests.
- A new answer form: front ends show chances, and the CLI's JSON writes them (as a fraction and a float). Other answers keep certain and candidate.
- The fraction's parts are `u128`: the denominators grow as chances multiply (2²⁴ for float draws, list sizes, feature counts), still far below its range for vanilla's data. A data pack with large weights and many features could overflow it; the arithmetic is checked, so that stops the query rather than giving a wrong chance.

## Alternatives considered

- **Certain or candidate only:** loses the difference between a 25% and an 8% flower. Plains, meadows and birch forests would answer only "one of these".
- **Chances as floats:** simpler, but sums aren't exact, and a search's "at least 25%" would depend on rounding at the boundary.
- **Chances by sampling the providers:** needs no derivation per provider, but it's slower and only approximate, and it duplicates what the oracle already checks.
