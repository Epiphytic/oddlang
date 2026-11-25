// Prepare a scratch client before compiling any adapter source.
package main

import (
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
)

func run() error {
	var pin struct{ Version string }
	var target struct{ Revision, SourceSha256 string }
	for file, out := range map[string]any{"client.lock.json": &pin, "target.lock.json": &target} {
		data, err := os.ReadFile(file)
		if err != nil {
			return err
		}
		if err = json.Unmarshal(data, out); err != nil {
			return err
		}
	}
	if !filepath.IsLocal(target.Revision) || filepath.Base(target.Revision) != target.Revision {
		return fmt.Errorf("invalid revision")
	}
	if pin.Version != "v1" && pin.Version != "v2" {
		return fmt.Errorf("invalid release version")
	}
	source := "client.go"
	if os.Args[1] == "restore" {
		source = filepath.Join(os.Args[2], target.Revision, "client.go")
	}
	data, err := os.ReadFile(source)
	if err != nil {
		return err
	}
	if fmt.Sprintf("%x", sha256.Sum256(data)) != target.SourceSha256 {
		return fmt.Errorf("digest mismatch for %s", source)
	}
	if os.Args[1] == "restore" {
		if err = os.WriteFile("client.go", data, 0600); err != nil {
			return err
		}
	}
	files, err := filepath.Glob("*.go")
	if err != nil {
		return err
	}
	for _, f := range files {
		if f != "client.go" && f != "client_test.go" && f != "lock_test.go" {
			return fmt.Errorf("unreviewed source %s", f)
		}
	}
	fmt.Println(pin.Version)
	return nil
}
func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
