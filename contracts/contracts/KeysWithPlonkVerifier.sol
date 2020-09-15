
pragma solidity >=0.5.0 <0.7.0;

import "./PlonkCore.sol";

// Hardcoded constants to avoid accessing store
contract KeysWithPlonkVerifier is VerifierWithDeserialize {

    function isBlockSizeSupportedInternal(uint32 _size) internal pure returns (bool) {
        if (_size == uint32(8)) { return true; }
        else if (_size == uint32(32)) { return true; }
        else { return false; }
    }

    function getVkBlock(uint32 _chunks) internal pure returns (VerificationKey memory vk) {
        if (_chunks == uint32(8)) { return getVkBlock8(); }
        else if (_chunks == uint32(32)) { return getVkBlock32(); }
    }

    
    function getVkBlock8() internal pure returns(VerificationKey memory vk) {
        vk.domain_size = 2097152;
        vk.num_inputs = 1;
        vk.omega = PairingsBn254.new_fr(0x032750f8f3c2493d0828c7285d0258e1bdcaa463f4442a52747b5c96639659bb);
        vk.selector_commitments[0] = PairingsBn254.new_g1(
            0x0eb582a83658e10f46f74a371360a40e4b23e23c2cbbb5ae6b0dc25a84fdb24b,
            0x19569028d244c212549df6c9d58bbb04e72b8218b7ad7fdc581a4db4860be9a9
        );
        vk.selector_commitments[1] = PairingsBn254.new_g1(
            0x0983d500a18a842c68bf8e3f6b6a08e499b82ed79feae5ca1660622660bbd189,
            0x1b6f3807c748a910e5a4235a64b97592fc90a147f3a15ee7b0ee800af0dc45dd
        );
        vk.selector_commitments[2] = PairingsBn254.new_g1(
            0x03723d73d210c38d47220473e0bf64c7b3fdfd0b48bdc1e1f2ea16c1d201c23b,
            0x1d7af92eb711eb90c42c9ccf4514f5872ff4ad3f4f58946d5ed7f9bbdf6f8fd3
        );
        vk.selector_commitments[3] = PairingsBn254.new_g1(
            0x1f5b7b47f00f4e9c44dcd24d8989e357763e06e626dbd5466df8fba296d4bc5b,
            0x2251507846a6bd241559d5cd1baca5f6806848b4c1ef3e91d25a9ec5a1c098cf
        );
        vk.selector_commitments[4] = PairingsBn254.new_g1(
            0x1bf08a2fb0b9ea00b370e991ab3fa589806ea103137035aba5551b670dc57231,
            0x22870bd4b1c76a8cacf14101c71aff92b587698d58216ba687a263c72702ce7c
        );
        vk.selector_commitments[5] = PairingsBn254.new_g1(
            0x23848b60f0c35a2e3d089a92908afc17057a66a91ce34a582205999dc1c3c074,
            0x2a21678b44dbbbaf71dd49cd3593dabea2cffb7558c38cfb6c9b95289de7d206
        );

        // we only have access to value of the d(x) witness polynomial on the next
        // trace step, so we only need one element here and deal with it in other places
        // by having this in mind
        vk.next_step_selector_commitments[0] = PairingsBn254.new_g1(
            0x1602c9fcb35c36913252d0d23f4115795c05ff71051e81d579a37ffb8decb55f,
            0x0f69d78e6ce1f08a40f0bdb4d3c24248949f32cecd89e0f3ed56079495bfa03e
        );

         vk.permutation_commitments[0] = PairingsBn254.new_g1(
            0x21e59911236245277e7de3799fbfbdf268d640b01cbcb1e670f6513c1a6b809a,
            0x0ccef1c18071945d25006aee244bf7a5b7bccbe8b07372c9362237f209d9093a
        );
        vk.permutation_commitments[1] = PairingsBn254.new_g1(
            0x2df9b2b70c7102e0879f5133be14ac52ae61bb7d4fcad09447c0abd00d19b050,
            0x2f1eb53fa8712f7e109c25848848f301300342c91159aed69d11aa641472587e
        );
        vk.permutation_commitments[2] = PairingsBn254.new_g1(
            0x24923b51ce42c61e9dd717cabb0eb42ce4ba5ca9b28fe0a5fa3c3427da435cc6,
            0x1ead7fe110b8bd24e5b77c88b2e8d6d6bda9c79a39b6b602bbb2a9160650e335
        );
        vk.permutation_commitments[3] = PairingsBn254.new_g1(
            0x0d5115626d3ef66a72463287ea17e4ba18b9cb6d37ccdd8af502fb3761772e0e,
            0x0c26199003d44167212afa171c8938bcec38d895b7932c5744392b697ccbc2c6
        );

        vk.permutation_non_residues[0] = PairingsBn254.new_fr(
            0x0000000000000000000000000000000000000000000000000000000000000005
        );
        vk.permutation_non_residues[1] = PairingsBn254.new_fr(
            0x0000000000000000000000000000000000000000000000000000000000000007
        );
        vk.permutation_non_residues[2] = PairingsBn254.new_fr(
            0x000000000000000000000000000000000000000000000000000000000000000a
        );

        vk.g2_x = PairingsBn254.new_g2(
            [0x260e01b251f6f1c7e7ff4e580791dee8ea51d87a358e038b4efe30fac09383c1,
             0x0118c4d5b837bcc2bc89b5b398b5974e9f5944073b32078b7e231fec938883b0],
            [0x04fc6369f7110fe3d25156c1bb9a72859cf2a04641f99ba4ee413c80da6a5fe4,
             0x22febda3c0c0632a56475b4214e5615e11e6dd3f96e6cea2854a87d4dacc5e55]
        );
    }
    
    function getVkBlock32() internal pure returns(VerificationKey memory vk) {
        vk.domain_size = 4194304;
        vk.num_inputs = 1;
        vk.omega = PairingsBn254.new_fr(0x18c95f1ae6514e11a1b30fd7923947c5ffcec5347f16e91b4dd654168326bede);
        vk.selector_commitments[0] = PairingsBn254.new_g1(
            0x23169466aa1323b0935a0a11038549a283f1f4a402c773efb65a39567e1f8974,
            0x015c59cbbc32aa5289ef17ddb3fa72e83b98b029e1939e94059af9ab46f18e81
        );
        vk.selector_commitments[1] = PairingsBn254.new_g1(
            0x255434ece8e4d1b60c908bf626259fab3de8eca44f0ff9ee84e8c56639be3d11,
            0x1f8d7d1e8aef0d5728fb44a9cc524a78f5197e8bbd521b85c7c0feecae4250ac
        );
        vk.selector_commitments[2] = PairingsBn254.new_g1(
            0x11fe6593e60553fb7f81708e1a6d72ebc84d8c8988c3e6a8af57057e23adcebc,
            0x1659dd494aef32f47b515c03a381f66ec9dd767eb9a7382bf9899f9da729cdff
        );
        vk.selector_commitments[3] = PairingsBn254.new_g1(
            0x2af4daabce0245a5a2b2e2f6a42f728fd01a8656c9b2175429764a48eba2beb5,
            0x1ee7d16a276b75a0f378eaa75f3ed3bb63d6d7667c3009515fb7f2571023b160
        );
        vk.selector_commitments[4] = PairingsBn254.new_g1(
            0x0772602382f7d46b30997b399957bda70bc85f06583cb0c2b7720e9d8beea552,
            0x2f9868f4b4f1555d4f5c7d39bc4e3ab39718f2a9a412a2ba579de00202df46cf
        );
        vk.selector_commitments[5] = PairingsBn254.new_g1(
            0x27a6e80d050511aa857f269debc28773db6f5ceda13a0db2df43fc7b2ac5b689,
            0x21e02b6a72e09424288e1cc34d667c8904bb8e02086454beb194623c23077291
        );

        // we only have access to value of the d(x) witness polynomial on the next
        // trace step, so we only need one element here and deal with it in other places
        // by having this in mind
        vk.next_step_selector_commitments[0] = PairingsBn254.new_g1(
            0x27d52653fed4978f2d52fae9d53e4d418a6b5516748d1d34fac48ceac0a82047,
            0x2df3d243cb5a408d9195804ad98f3bbf60ac4d9e3e65f12dd36506354874482b
        );

         vk.permutation_commitments[0] = PairingsBn254.new_g1(
            0x1ee111613e83ea259c4979200693029eb052d14f938b84c38c7df63c5ae92cd8,
            0x00454e0ed203952ebb65d065b2281f7a65b7c798c538d2d40373aba461820e0e
        );
        vk.permutation_commitments[1] = PairingsBn254.new_g1(
            0x0a284926ff817dc66e35a7733b1e850ee4fc061d2ecbc464e2ca585702ef1e80,
            0x01ab1adedc58ae54352ef632988c1963e12bd81ef16b359aa86fc59b371ab444
        );
        vk.permutation_commitments[2] = PairingsBn254.new_g1(
            0x0c4cc0e374623c225ca116418f3b26b41ad806fe26a5b366d87410b60fc1f8fe,
            0x09e5405b51df8b13ccfd53aba5cbdada2bc989ebb1fa5949548c569e1f1d0acc
        );
        vk.permutation_commitments[3] = PairingsBn254.new_g1(
            0x1d43fa27f6c71606c8c2a81910bf27a7345acba33139f3f9c3ff8a7ec533744b,
            0x070bbca01fb1288cee6d1eb5308fa7e68d0ac485622fa9679c463ad0ca74402a
        );

        vk.permutation_non_residues[0] = PairingsBn254.new_fr(
            0x0000000000000000000000000000000000000000000000000000000000000005
        );
        vk.permutation_non_residues[1] = PairingsBn254.new_fr(
            0x0000000000000000000000000000000000000000000000000000000000000007
        );
        vk.permutation_non_residues[2] = PairingsBn254.new_fr(
            0x000000000000000000000000000000000000000000000000000000000000000a
        );

        vk.g2_x = PairingsBn254.new_g2(
            [0x260e01b251f6f1c7e7ff4e580791dee8ea51d87a358e038b4efe30fac09383c1,
             0x0118c4d5b837bcc2bc89b5b398b5974e9f5944073b32078b7e231fec938883b0],
            [0x04fc6369f7110fe3d25156c1bb9a72859cf2a04641f99ba4ee413c80da6a5fe4,
             0x22febda3c0c0632a56475b4214e5615e11e6dd3f96e6cea2854a87d4dacc5e55]
        );
    }
    

}
