package client

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
)

type contractPin struct{ Identity, Version, ManifestSha256 string }
type targetPin struct{ Revision, SourceSha256, ManifestSha256, OverlaySha256, VectorsSha256 string }

func readJSON(file string, out any) error {
	b, err := os.ReadFile(file)
	if err != nil {
		return err
	}
	return json.Unmarshal(b, out)
}

func checkedFile(path, want string) ([]byte, error) {
	b, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	hash := sha256.Sum256(b)
	if hex.EncodeToString(hash[:]) != want {
		return nil, fmt.Errorf("digest mismatch for %s", path)
	}
	return b, nil
}

func verifiedVectors() (string, []byte, error) {
	var pin contractPin
	var target targetPin
	if err := readJSON("client.lock.json", &pin); err != nil {
		return "", nil, err
	}
	if err := readJSON("target.lock.json", &target); err != nil {
		return "", nil, err
	}
	if target.ManifestSha256 != pin.ManifestSha256 || pin.Identity != "urn:oddlang:contract:ledger" {
		return "", nil, fmt.Errorf("target contract mismatch")
	}
	if !filepath.IsLocal(target.Revision) {
		return "", nil, fmt.Errorf("invalid revision")
	}
	for _, file := range []string{filepath.Join("revisions", target.Revision, "client.go"), "client.go"} {
		if _, err := checkedFile(file, target.SourceSha256); err != nil {
			return "", nil, err
		}
	}
	manifest, err := checkedFile("bundle/manifest.json", pin.ManifestSha256)
	if err != nil {
		return "", nil, err
	}
	var m struct {
		Identity, Version string
		Files             map[string]string
	}
	if err = json.Unmarshal(manifest, &m); err != nil {
		return "", nil, err
	}
	if m.Identity != pin.Identity || m.Version != pin.Version || m.Files["wire.json"] != target.OverlaySha256 || m.Files["vectors.json"] != target.VectorsSha256 || target.OverlaySha256 == "" || target.VectorsSha256 == "" {
		return "", nil, fmt.Errorf("target inputs mismatch")
	}
	var vectors []byte
	for path, digest := range m.Files {
		if !filepath.IsLocal(path) {
			return "", nil, fmt.Errorf("invalid bundle path %q", path)
		}
		data, err := checkedFile(filepath.Join("bundle", path), digest)
		if err != nil {
			return "", nil, err
		}
		if path == "vectors.json" {
			vectors = data
		}
	}
	return pin.Version, vectors, nil
}
