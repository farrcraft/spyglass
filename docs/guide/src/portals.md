# Nether portals

Going through a Nether portal takes you to a portal that's already there, if one is near enough, and otherwise the game builds a new one. Which portal you land at depends only on where you stand when you go through, and on where the portals are. spyglass works that out exactly, for portals you mark and for the ones your world already has, so you can find a mislinked pair, or move a portal, without walking back and forth to test it.

## How the game picks a portal

- **Where you aim.** Your position is scaled: divided by 8 going into the Nether, times 8 coming out. y stays as it is. Coming out of the Nether, a 2 × 3 portal spans about 12 by 14 blocks of the Overworld, so where you stand in it matters.
- **The search.** The game looks for portal blocks within 16 blocks of that spot in the Nether, or 128 in the Overworld, in x and z, at any height. The nearest wins, counting height too, and then the lowest.
- **None in range:** the game builds a new portal near the spot. spyglass doesn't say where it would build it, since that depends on the blocks there.

A portal "links back" when trips from each portal land at the other. Two portals that are both in range of a third, or a portal built where its partner's search reaches another one first, break that.

## Checking a portal

```sh
spyglass portal link nether 100,64,100 --portal overworld:800,64,800 --game-version 26.3
```

Give the portal's dimension and its lowest block, and with `--axis z` and `--size 4x5` its direction and size inside the frame if they aren't along x and 2 × 3. The answer lists the blocks a player standing in it aims at, and where each part of it lands among the portals you give with `--portal`, and with `--world`, your world's portals:

```text
A 2 × 3 portal along x at 100 64 100 in the Nether leads to the Overworld (Minecraft 26.3):
  336 exit blocks, x 802 to 813, y 64 to 65, z 797 to 810, each searched 128 blocks around in x and z
  all exits land at --portal 1 (the Overworld 800 64 800)
```

A portal can be **split**: some places in it lead to one portal and some to another, or to none. That's usually a sign to move one of them.

## Your world's portals

```sh
spyglass portal link --world "My World"
```

This lists every portal your world has, in each dimension, with where it leads, and flags the ones that need a look:

- **mislinked:** it leads to a portal that doesn't lead back to it;
- **split:** different places in it lead to different portals;
- **unlinked:** no portal is in range on the other side, so going through builds a new one;
- **tied:** two portals are exactly as near, and which one you land at can't be known from the portals alone.

spyglass reads the portals from the world's points-of-interest files, where the game keeps them. Parts of a world last played before 1.16 may have portals those files don't list yet, and spyglass says so.

In the viewer, open the save: its portals are drawn in both dimensions, in purple, and flagged ones in orange. Hover one for where it leads and what's wrong.

## Moving a portal without breaking its link

Say your base's Nether portal links to a hub portal in the Nether, and you want to move it.

1. In the viewer, add a **portal marker** where the hub portal is: right-click the map in the Nether and **Add a portal here**, then set its height, direction and size in the marker's window. Or open your save, whose portals are already there.
2. Right-click the hub portal and **Show its link area**. The map switches to the Overworld and shades it: green where a portal you build links to the hub, red where it would find none and build a new one, dark where it links to another portal, and yellow where it depends where you stand in it or ties.
3. Build anywhere inside the green, at the height shown, and the link holds. The shading is for the view's height, since the search counts height too: change the height to check another.

The other direction matters too: the hub's own trips must land at your new portal. Add a marker for the new portal and switch to the Nether: the hub's search square, and a line to the portal it lands at, show whether it comes back.
