// Independent codec for the pinned shapes example. It uses the case protocol
// and Go's standard library and has no Oddlang or generated dependency.
package main

import (
	"bytes"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"strconv"
	"unicode/utf8"
)

type witness struct {
	ID        string `json:"caseId"`
	Operation string `json:"operation"`
	Action    struct {
		Body string `json:"bodyBase64"`
	} `json:"action"`
}

func value(decoder *json.Decoder) (any, error) {
	first, err := decoder.Token()
	if err != nil {
		return nil, err
	}
	if first == json.Delim('{') {
		fields := map[string]any{}
		for decoder.More() {
			name, err := decoder.Token()
			if err != nil {
				return nil, err
			}
			key, ok := name.(string)
			if !ok {
				return nil, fmt.Errorf("invalid member name")
			}
			if _, duplicate := fields[key]; duplicate {
				return nil, fmt.Errorf("duplicate member")
			}
			fields[key], err = value(decoder)
			if err != nil {
				return nil, err
			}
		}
		_, err := decoder.Token()
		return fields, err
	}
	if first == json.Delim('[') {
		items := []any{}
		for decoder.More() {
			item, err := value(decoder)
			if err != nil {
				return nil, err
			}
			items = append(items, item)
		}
		_, err := decoder.Token()
		return items, err
	}
	switch first.(type) {
	case nil, string, bool:
		return first, nil
	default:
		return nil, fmt.Errorf("unsupported JSON value")
	}
}

// encoding/json replaces isolated surrogate escapes, so check pairing first.
func unicodeEscapes(body []byte) bool {
	for i := 0; i < len(body); i++ {
		if body[i] != '\\' {
			continue
		}
		i++
		if i == len(body) {
			return false
		}
		if body[i] != 'u' {
			continue
		}
		if i+4 >= len(body) {
			return false
		}
		unit, err := strconv.ParseUint(string(body[i+1:i+5]), 16, 16)
		i += 4
		if err != nil || unit >= 0xdc00 && unit <= 0xdfff {
			return false
		}
		if unit >= 0xd800 && unit <= 0xdbff {
			if i+6 >= len(body) || string(body[i+1:i+3]) != "\\u" {
				return false
			}
			low, err := strconv.ParseUint(string(body[i+3:i+7]), 16, 16)
			if err != nil || low < 0xdc00 || low > 0xdfff {
				return false
			}
			i += 6
		}
	}
	return true
}

func valid(decoded any) bool {
	root, ok := decoded.(map[string]any)
	if !ok || len(root) < 4 || len(root) > 5 {
		return false
	}
	for key := range root {
		switch key {
		case "metadata", "items", "choice", "memo", "note":
		default:
			return false
		}
	}
	metadata, ok := root["metadata"].(map[string]any)
	if !ok || len(metadata) != 1 {
		return false
	}
	if _, ok := metadata["enabled"].(bool); !ok {
		return false
	}
	items, ok := root["items"].([]any)
	if !ok {
		return false
	}
	for _, item := range items {
		wire, ok := item.(string)
		n, err := strconv.ParseInt(wire, 10, 64)
		if !ok || err != nil || strconv.FormatInt(n, 10) != wire {
			return false
		}
	}
	if memo, present := root["memo"]; present {
		if _, ok := memo.(string); !ok {
			return false
		}
	}
	note, present := root["note"]
	if !present {
		return false
	}
	if note != nil {
		if _, ok := note.(string); !ok {
			return false
		}
	}
	choice, ok := root["choice"].(map[string]any)
	if !ok || len(choice) != 2 {
		return false
	}
	switch choice["kind"] {
	case "named":
		_, ok = choice["name"].(string)
	case "flagged":
		_, ok = choice["flag"].(bool)
	default:
		return false
	}
	return ok
}

func observe(operation string, body []byte) map[string]any {
	reject := map[string]any{"kind": "localRejection"}
	if operation != "urn:oddlang:shapes:RoundTrip" || !utf8.Valid(body) || !unicodeEscapes(body) {
		return reject
	}
	decoder := json.NewDecoder(bytes.NewReader(body))
	decoder.UseNumber()
	root, err := value(decoder)
	if err != nil || !valid(root) {
		return reject
	}
	if _, err := decoder.Token(); err != io.EOF {
		return reject
	}
	return map[string]any{"kind": "success", "value": root}
}

func run() error {
	var cases []witness
	if err := json.NewDecoder(os.Stdin).Decode(&cases); err != nil {
		return err
	}
	results := make([]any, 0, len(cases))
	for _, c := range cases {
		body, err := base64.StdEncoding.DecodeString(c.Action.Body)
		if err != nil {
			return err
		}
		results = append(results, map[string]any{
			"caseId": c.ID, "outcome": "pass",
			"observations": []any{map[string]any{"native": observe(c.Operation, body)}},
		})
	}
	return json.NewEncoder(os.Stdout).Encode(results)
}
func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
