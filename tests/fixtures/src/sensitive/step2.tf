resource "terraform_data" "leaf" {
  input = {
    label    = "db"
    password = sensitive("CANARY-leaf-after-9c1d")
    endpoint = "DefaultEndpointsProtocol=https;AccountName=acct;AccountKey=CANARYkeyAFTERz9x8c7v6b5n4m3a2s1d0=="
    api      = "CANARYhighEntropyAFTER4tG8nBv2Xc6Jm1Qs9Wd3Fk7Lp5"
    users = [
      { name = "bob", token = sensitive("CANARY-bob-after-33cc") },
      { name = "alice", token = sensitive("CANARY-alice-before-11aa") },
    ]
  }
}

resource "terraform_data" "block" {
  input = sensitive([
    { name = "CANARY-block-name-one", value = "CANARY-block-value-one" },
    { name = "CANARY-block-name-two", value = "CANARY-block-value-changed" },
    { name = "CANARY-block-name-three", value = "CANARY-block-value-three" },
  ])
}
