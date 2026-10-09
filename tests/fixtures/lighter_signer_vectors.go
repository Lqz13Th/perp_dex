// Golden vectors for the pure-Rust Lighter signer (fixed test seed, not a real account key), generated with the
// official Go signer. Copy into a github.com/elliottech/lighter-go checkout as cmd/golden/main.go, then:
//
//	go run ./cmd/golden > lighter_signer_vectors.json   hashes, tx_info, official signatures, auth token
//	go run ./cmd/golden verify < rust_sigs.json         checks Rust-made signatures with the official verifier
package main

import (
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"time"

	"github.com/elliottech/lighter-go/signer"
	"github.com/elliottech/lighter-go/types"
	"github.com/elliottech/lighter-go/types/txtypes"
	g "github.com/elliottech/poseidon_crypto/field/goldilocks"
	p2g "github.com/elliottech/poseidon_crypto/hash/poseidon2_goldilocks"
	schnorr "github.com/elliottech/poseidon_crypto/signature/schnorr"
)

const seed = "6c69676874657220676f6c64656e20766563746f7273202d2074657374206f6e6c79"
const chainId = uint32(304)

type vector struct {
	Name      string `json:"name"`
	TxType    uint8  `json:"tx_type"`
	TxInfo    string `json:"tx_info"`
	Hash      string `json:"hash"`
	Signature string `json:"signature"`
}

type out struct {
	PrivateKey string   `json:"private_key"`
	PublicKey  string   `json:"public_key"`
	ChainId    uint32   `json:"chain_id"`
	Txs        []vector `json:"txs"`
	AuthMsg    string   `json:"auth_message"`
	AuthHash   string   `json:"auth_hash"`
	AuthToken  string   `json:"auth_token"`
}

type txi interface {
	GetTxType() uint8
	GetTxInfo() (string, error)
	GetTxHash() string
}

func must[T any](v T, err error) T {
	if err != nil {
		panic(err)
	}
	return v
}

func main() {
	key := must(signer.NewSeedKeyManager(seed))
	if len(os.Args) > 1 && os.Args[1] == "verify" {
		verify(key)
		return
	}
	acc, idx := int64(758666), uint8(4)
	ops := func(nonce int64) *types.TransactOpts {
		n := nonce
		return &types.TransactOpts{FromAccountIndex: &acc, ApiKeyIndex: &idx, ExpiredAt: 1791520000000, Nonce: &n}
	}
	o := out{PrivateKey: hex.EncodeToString(key.PrvKeyBytes()), ChainId: chainId}
	pk := key.PubKeyBytes()
	o.PublicKey = hex.EncodeToString(pk[:])
	add := func(name string, tx txi) {
		info := must(tx.GetTxInfo())
		var m map[string]any
		_ = json.Unmarshal([]byte(info), &m)
		sig := ""
		if s, ok := m["Sig"].(string); ok {
			sig = s
		}
		o.Txs = append(o.Txs, vector{Name: name, TxType: tx.GetTxType(), TxInfo: info, Hash: tx.GetTxHash(), Signature: sig})
	}
	order := func(name string, nonce int64, r types.CreateOrderTxReq) {
		add(name, must(types.ConstructCreateOrderTx(key, chainId, &r, ops(nonce))))
	}
	order("limit_post_only_buy", 7, types.CreateOrderTxReq{MarketIndex: 139, ClientOrderIndex: 1791518274, BaseAmount: 72, Price: 155296,
		IsAsk: 0, Type: txtypes.LimitOrder, TimeInForce: txtypes.PostOnly, OrderExpiry: 1794110274000})
	order("limit_gtt_sell", 8, types.CreateOrderTxReq{MarketIndex: 139, ClientOrderIndex: 42, BaseAmount: 35, Price: 170000,
		IsAsk: 1, Type: txtypes.LimitOrder, TimeInForce: txtypes.GoodTillTime, OrderExpiry: 1794110274000})
	order("ioc_buy", 9, types.CreateOrderTxReq{MarketIndex: 3, ClientOrderIndex: 281474976710655, BaseAmount: 123456, Price: 4294967295,
		IsAsk: 0, Type: txtypes.LimitOrder, TimeInForce: txtypes.ImmediateOrCancel, OrderExpiry: txtypes.NilOrderExpiry})
	order("ioc_reduce_only_sell", 10, types.CreateOrderTxReq{MarketIndex: 0, ClientOrderIndex: 0, BaseAmount: 1, Price: 1,
		IsAsk: 1, Type: txtypes.LimitOrder, TimeInForce: txtypes.ImmediateOrCancel, ReduceOnly: 1, OrderExpiry: txtypes.NilOrderExpiry})
	add("cancel_by_order_index", must(types.ConstructL2CancelOrderTx(key, chainId, &types.CancelOrderTxReq{MarketIndex: 139, Index: 39687971468506323}, ops(11))))
	add("cancel_by_client_index", must(types.ConstructL2CancelOrderTx(key, chainId, &types.CancelOrderTxReq{MarketIndex: 139, Index: 1791518274}, ops(12))))
	add("update_leverage_isolated_5x", must(types.ConstructUpdateLeverageTx(key, chainId, &types.UpdateLeverageTxReq{MarketIndex: 139, InitialMarginFraction: 2000, MarginMode: txtypes.IsolatedMargin}, ops(13))))
	add("update_margin_add", must(types.ConstructUpdateMarginTx(key, chainId, &types.UpdateMarginTxReq{MarketIndex: 139, USDCAmount: 5_000_000, Direction: txtypes.AddToIsolatedMargin}, ops(14))))
	add("update_margin_large", must(types.ConstructUpdateMarginTx(key, chainId, &types.UpdateMarginTxReq{MarketIndex: 2, USDCAmount: 9_000_000_000_123, Direction: txtypes.RemoveFromIsolatedMargin}, ops(15))))
	add("cancel_all_immediate", must(types.ConstructL2CancelAllOrdersTx(key, chainId, &types.CancelAllOrdersTxReq{TimeInForce: txtypes.ImmediateCancelAll, Time: 0}, ops(16))))
	add("modify_by_order_index", must(types.ConstructL2ModifyOrderTx(key, chainId, &types.ModifyOrderTxReq{MarketIndex: 139, Index: 39687971468506323, BaseAmount: 71, Price: 156215, TriggerPrice: 0}, ops(17))))
	add("modify_by_client_index", must(types.ConstructL2ModifyOrderTx(key, chainId, &types.ModifyOrderTxReq{MarketIndex: 3, Index: 281474976710655, BaseAmount: 123456, Price: 4294967295, TriggerPrice: 0}, ops(18))))

	deadline := time.Unix(1791520000, 0)
	o.AuthMsg = fmt.Sprintf("%v:%v:%v", deadline.Unix(), acc, idx)
	tok := must(types.ConstructAuthToken(key, deadline, &types.TransactOpts{FromAccountIndex: &acc, ApiKeyIndex: &idx}))
	o.AuthToken = tok
	o.AuthHash = authHash(o.AuthMsg)
	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", "  ")
	_ = enc.Encode(o)
}

func authHash(message string) string {
	elems := must(g.ArrayFromCanonicalLittleEndianBytes([]byte(message)))
	h := p2g.HashToQuinticExtension(elems).ToLittleEndianBytes()
	return hex.EncodeToString(h)
}

type sigItem struct {
	Name      string `json:"name"`
	Hash      string `json:"hash"`
	Signature string `json:"signature"`
}

func verify(key signer.KeyManager) {
	var in struct {
		PublicKey string    `json:"public_key"`
		Items     []sigItem `json:"items"`
	}
	if err := json.NewDecoder(os.Stdin).Decode(&in); err != nil {
		panic(err)
	}
	pk := must(hex.DecodeString(in.PublicKey))
	ours := key.PubKeyBytes()
	fmt.Printf("public key matches seed key: %v\n", hex.EncodeToString(ours[:]) == in.PublicKey)
	bad := 0
	for _, it := range in.Items {
		h := must(hex.DecodeString(it.Hash))
		s := must(hex.DecodeString(it.Signature))
		if err := schnorr.Validate(pk, h, s); err != nil {
			bad++
			fmt.Printf("%s: INVALID %v\n", it.Name, err)
		} else {
			fmt.Printf("%s: ok\n", it.Name)
		}
	}
	fmt.Printf("%d/%d valid\n", len(in.Items)-bad, len(in.Items))
}
