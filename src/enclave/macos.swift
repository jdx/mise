// Seals a secret to this Mac's Secure Enclave so unsealing needs Touch ID.
//
//   mise-enclave seal    stdin: secret   stdout: three base64 lines
//   mise-enclave unseal  stdin: those lines   stdout: secret
//
// The secret never appears in argv. Each seal creates a fresh enclave key; its
// opaque `dataRepresentation` is part of the output, so nothing is stored in the
// keychain and no entitlement or provisioning profile is needed. The blob only
// works on this Mac, and the enclave enforces the biometric check on every use.
import CryptoKit
import Foundation
import LocalAuthentication

let info = Data("dev.jdx.mise.enclave.v1".utf8)

func fail(_ message: String, _ code: Int32 = 1) -> Never {
    FileHandle.standardError.write(Data("mise-enclave: \(message)\n".utf8))
    exit(code)
}

func derive(_ secret: SharedSecret) -> SymmetricKey {
    secret.hkdfDerivedSymmetricKey(
        using: SHA256.self, salt: Data(), sharedInfo: info, outputByteCount: 32)
}

guard CommandLine.arguments.count == 2 else { fail("usage: mise-enclave seal|unseal", 2) }
let input = FileHandle.standardInput.readDataToEndOfFile()

do {
    switch CommandLine.arguments[1] {
    case "seal":
        var error: Unmanaged<CFError>?
        guard
            let access = SecAccessControlCreateWithFlags(
                nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
                [.privateKeyUsage, .biometryCurrentSet], &error)
        else { fail("could not create the access control: \(String(describing: error))") }
        let key = try SecureEnclave.P256.KeyAgreement.PrivateKey(accessControl: access)
        let ephemeral = P256.KeyAgreement.PrivateKey()
        let shared = try ephemeral.sharedSecretFromKeyAgreement(with: key.publicKey)
        let box = try AES.GCM.seal(input, using: derive(shared))
        print(key.dataRepresentation.base64EncodedString())
        print(ephemeral.publicKey.x963Representation.base64EncodedString())
        print(box.combined!.base64EncodedString())
    case "unseal":
        let lines = String(decoding: input, as: UTF8.self)
            .split(whereSeparator: \.isNewline)
            .compactMap { Data(base64Encoded: String($0)) }
        guard lines.count == 3 else { fail("the sealed value is malformed") }
        let context = LAContext()
        context.localizedReason = "unlock the mise GitHub OAuth refresh token"
        let key = try SecureEnclave.P256.KeyAgreement.PrivateKey(
            dataRepresentation: lines[0], authenticationContext: context)
        let ephemeral = try P256.KeyAgreement.PublicKey(x963Representation: lines[1])
        let shared = try key.sharedSecretFromKeyAgreement(with: ephemeral)
        let secret = try AES.GCM.open(AES.GCM.SealedBox(combined: lines[2]), using: derive(shared))
        FileHandle.standardOutput.write(secret)
    case "--check":
        // Prove the enclave is usable without prompting: keys are only created
        // by `seal`, which needs no biometric.
        guard SecureEnclave.isAvailable else { fail("no Secure Enclave on this Mac") }
    default:
        fail("usage: mise-enclave seal|unseal", 2)
    }
} catch {
    fail("\(error)")
}
