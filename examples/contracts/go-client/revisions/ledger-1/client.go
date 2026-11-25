package client

import (
	"encoding/json"
	"fmt"
	"math"
	"strconv"
)

type Decimal int64

func (d *Decimal) UnmarshalJSON(raw []byte) error {
	var s string
	if err := json.Unmarshal(raw, &s); err != nil {
		return err
	}
	n, err := strconv.ParseInt(s, 10, 64)
	if err != nil || strconv.FormatInt(n, 10) != s {
		return fmt.Errorf("invalid decimal %q", s)
	}
	*d = Decimal(n)
	return nil
}

type Account struct {
	ID           string
	Owner        string
	Balance      Decimal
	CurrencyCode string
}

func decodeAccount(version string, raw []byte) (Account, error) {
	var a Account
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(raw, &fields); err != nil {
		return a, err
	}
	required := []string{"id", "owner", "balance"}
	switch version {
	case "v2":
		required = append(required, "currencyCode")
	case "v1":
	default:
		return a, fmt.Errorf("unsupported account version %q", version)
	}
	for _, key := range required {
		if len(fields[key]) == 0 || string(fields[key]) == "null" {
			return a, fmt.Errorf("missing %s", key)
		}
	}
	if err := json.Unmarshal(raw, &a); err != nil {
		return a, err
	}
	if a.Balance < 0 {
		return a, fmt.Errorf("negative balance")
	}
	return a, nil
}

type Request struct {
	Source, Destination string
	Amount              Decimal
}
type Result struct {
	OK       bool
	Balances map[string]string
	Error    string
}

func transfer(accounts []Account, req Request) Result {
	balances := make(map[string]Decimal, len(accounts))
	for _, a := range accounts {
		balances[a.ID] = a.Balance
	}
	src, hasSrc := balances[req.Source]
	dst, hasDst := balances[req.Destination]
	var failure string
	switch {
	case !hasSrc:
		failure = "SourceMissing"
	case !hasDst:
		failure = "DestinationMissing"
	case req.Source == req.Destination:
		failure = "SameAccount"
	case req.Amount <= 0:
		failure = "NonPositiveAmount"
	case src < req.Amount:
		failure = "InsufficientFunds"
	case dst > math.MaxInt64-req.Amount:
		failure = "DestinationOverflow"
	default:
		return Result{OK: true, Balances: map[string]string{
			req.Source:      strconv.FormatInt(int64(src-req.Amount), 10),
			req.Destination: strconv.FormatInt(int64(dst+req.Amount), 10),
		}}
	}
	return Result{Error: "urn:oddlang:ledger:error:" + failure}
}
