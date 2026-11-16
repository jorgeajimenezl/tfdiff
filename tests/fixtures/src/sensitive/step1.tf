resource "terraform_data" "leaf" {
  input = {
    label    = "db"
    password = sensitive("CANARY-leaf-before-7f3a")
    endpoint = "DefaultEndpointsProtocol=https;AccountName=acct;AccountKey=CANARYkeyBEFOREq1w2e3r4t5y6u7i8o9p0=="
    api      = "CANARYhighEntropyBEFORE9xK2mPq7Lw4Zt8Rv3Ns6Yb1Hd5"
    users = [
      { name = "alice", token = sensitive("CANARY-alice-before-11aa") },
      { name = "bob", token = sensitive("CANARY-bob-before-22bb") },
    ]
  }
}

resource "terraform_data" "block" {
  input = sensitive([
    { name = "CANARY-block-name-one", value = "CANARY-block-value-one" },
    { name = "CANARY-block-name-two", value = "CANARY-block-value-two" },
  ])
}
