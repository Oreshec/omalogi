# Task prompts for coding agents

Omalogi is built agent first. Open the agent you prefer in a clone of this repository,
paste one of these prompts, and fill in the parts in angle brackets. Every prompt starts
from [agent-guide.md](agent-guide.md), which holds the project map, the checks and the hard
rules, so any model works the same way.

| Prompt | Use it to |
|---|---|
| [verify-my-mouse.md](verify-my-mouse.md) | Verify a mouse Omalogi lists as untested, and mark it verified |
| [add-a-device.md](add-a-device.md) | Add support for another Logitech mouse you own |
| [fix-a-bug.md](fix-a-bug.md) | Fix a reported bug, test first |
| [improve-the-overlay.md](improve-the-overlay.md) | Change or polish the Omarchy overlay or bar widget |
| [hardware-test.md](hardware-test.md) | Verify a change on a real mouse, safely, and log it |

Steps that touch a real mouse are always run by you, the human: the agent prepares the
exact commands and reads the results you paste back.

When the work is done, ask the agent to open a pull request that says what changed, how
it was tested, and which hardware steps you ran.
