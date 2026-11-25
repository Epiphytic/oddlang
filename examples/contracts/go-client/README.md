# Independent Go ledger client

Run `sh examples/contracts/go-client/verify.sh` from the repository root with Go 1.22 or newer. The driver copies the client, its own pins and one release bundle into a temporary module. Its Go tests use only the standard library and run with module downloads and toolchain downloads disabled.

The driver advances a separate publisher workspace pin to v2, runs the client again on v1, then installs the client's reviewed v2 pins and checks the v2 account projection. Both releases use the v1 single-currency transfer vectors. Balances and amounts cross the JSON boundary as decimal strings.

Each target lock identifies a recoverable source revision in `revisions/` and pins the source, release manifest, wire overlay (`wire.json`) and published vectors by SHA-256. Both target locks accept the same Go source. The driver changes the working copy, checks for the specific digest rejection, restores the accepted bytes and reruns the vectors offline.
