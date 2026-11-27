
import { Sky, Side, Tier } from "./values";
// @ts-expect-error An unchecked ordinal has no choice identity.
const wrong: Sky = 0;
// @ts-expect-error Choices with overlapping ordinals have distinct brands.
const mixed: Sky = Side.Left;
// @ts-expect-error Equality is absent on Sky.
Sky.equals(Sky.Clear, Sky.Cloud);
// @ts-expect-error Partial equality makes no total capability available.
Side.equals(Side.Left, Side.Right);
// @ts-expect-error Total equality still requires a pair of the same choice.
Tier.equals(Tier.Local, Side.Left);
// @ts-expect-error Exposed constants are immutable.
Sky.Clear = Sky.Cloud;
