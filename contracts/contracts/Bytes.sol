pragma solidity ^0.5.0;


library Bytes {

    function toBytesFromUInt16(uint16 self) internal pure returns (bytes memory _bts) {
        return toBytesFromUIntTruncated(uint(self), 2);
    }

    function toBytesFromUInt24(uint24 self) internal pure returns (bytes memory _bts) {
        return toBytesFromUIntTruncated(uint(self), 3);
    }

    function toBytesFromUInt32(uint32 self) internal pure returns (bytes memory _bts) {
        return toBytesFromUIntTruncated(uint(self), 4);
    }

    function toBytesFromUInt128(uint128 self) internal pure returns (bytes memory _bts) {
        return toBytesFromUIntTruncated(uint(self), 16);
    }

    function toBytesFromUInt256(uint256 self) internal pure returns (bytes memory _bts) {
        return toBytesFromUIntTruncated(uint(self), 32);
    }

    // Copies 'len' lower bytes from 'self' into a new 'bytes memory'.
    // Returns the newly created 'bytes memory'. The returned bytes will be of length 'len'.
    function toBytesFromUIntTruncated(uint self, uint8 byteLength) private pure returns (bytes memory bts) {
        require(byteLength <= 32, "bt211");
        bts = new bytes(byteLength);
        // Even though the bytes will allocate a full word, we don't want
        // any potential garbage bytes in there.
        uint data = self << ((32 - byteLength) * 8);
        assembly {
            mstore(add(bts, /*BYTES_HEADER_SIZE*/32), data)
        }
    }

    // Copies 'self' into a new 'bytes memory'.
    // Returns the newly created 'bytes memory'. The returned bytes will be of length '20'.
    function toBytesFromAddress(address self) internal pure returns (bytes memory bts) {
        bts = toBytesFromUIntTruncated(uint(self), 20);
    }

    function bytesToAddress(bytes memory self, uint256 _start) internal pure returns (address addr) {
        require(self.length >= (_start + 20), "bta11");
        assembly {
            addr := mload(add(add(self, 20), _start))
        }
    }

    function bytesToBytes20(bytes memory self, uint256 _start) internal pure returns (bytes20 r) {
        require(self.length >= (_start + 20), "btb20");
        assembly {
            // Note that bytes1..32 is stored in the beginning of the word unlike other primitive types
            r := mload(add(add(self, 0x20), _start))
        }
    }

    function bytesToUInt16(bytes memory _bytes, uint256 _start) internal pure returns (uint16 r) {
        require(_bytes.length >= (_start + 2), "btu02");
        assembly {
            r := mload(add(add(_bytes, 0x2), _start))
        }
    }

    function bytesToUInt24(bytes memory _bytes, uint256 _start) internal pure returns (uint24 r) {
        require(_bytes.length >= (_start + 3), "btu03");
        assembly {
            r := mload(add(add(_bytes, 0x3), _start))
        }
    }

    function bytesToUInt32(bytes memory _bytes, uint256 _start) internal pure returns (uint32 r) {
        require(_bytes.length >= (_start + 4), "btu04");
        assembly {
            r := mload(add(add(_bytes, 0x4), _start))
        }
    }

    function bytesToUInt128(bytes memory _bytes, uint256 _start) internal pure returns (uint128 r) {
        require(_bytes.length >= (_start + 16), "btu16");
        assembly {
            r := mload(add(add(_bytes, 0x10), _start))
        }
    }

    function bytesToUInt160(bytes memory _bytes, uint256 _start) internal pure returns (uint160 r) {
        require(_bytes.length >= (_start + 20), "btu20");
        assembly {
            r := mload(add(add(_bytes, 0x14), _start))
        }
    }

    function bytesToBytes32(bytes memory  _bytes, uint256 _start) internal pure returns (bytes32 r) {
        require(_bytes.length >= 0x20, "btb32");
        assembly {
            r := mload(add(add(_bytes, 0x20), _start))
        }
    }

    // Original source code: https://github.com/GNSPS/solidity-bytes-utils/blob/master/contracts/BytesLib.sol#L228
    // Get slice from bytes arrays
    // Returns the newly created 'bytes memory'
    function slice(
        bytes memory _bytes,
        uint _start,
        uint _length
    )
        internal
        pure
        returns (bytes memory)
    {
        require(_bytes.length >= (_start + _length), "bse11"); // bytes length is less then start byte + length bytes

        bytes memory tempBytes = new bytes(_length);

        if (_length != 0) {
            // TODO: Review this thoroughly.
            assembly {
                let slice_curr := add(tempBytes, 0x20)
                let slice_end := add(slice_curr, _length)

                for {
                    // The multiplication in the next line has the same exact purpose
                    // as the one above.
                    let array_current := add(_bytes, add(_start, 0x20))
                } lt(slice_curr, slice_end) {
                    slice_curr := add(slice_curr, 0x20)
                    array_current := add(array_current, 0x20)
                } {
                    mstore(slice_curr, mload(array_current))
                }
            }
        }

        return tempBytes;
    }

    /// Reads byte stream
    /// @return new_offset - offset + amount of bytes read
    /// @return data - actually read data
    function read(bytes memory _data, uint _offset, uint _length) internal pure returns (uint new_offset, bytes memory data) {
        data = slice(_data, _offset, _length);
        new_offset = _offset + _length;
    }

    function readBool(bytes memory _data, uint _offset) internal pure returns (uint new_offset, bool r) {
        new_offset = _offset + 1;
        r = uint8(_data[_offset]) != 0;
    }

    function readUint8(bytes memory _data, uint _offset) internal pure returns (uint new_offset, uint8 r) {
        new_offset = _offset + 1;
        r = uint8(_data[_offset]);
    }

    function readUInt16(bytes memory _data, uint _offset) internal pure returns (uint new_offset, uint16 r) {
        new_offset = _offset + 2;
        r = bytesToUInt16(_data, _offset);
    }

    function readUInt24(bytes memory _data, uint _offset) internal pure returns (uint new_offset, uint24 r) {
        new_offset = _offset + 3;
        r = bytesToUInt24(_data, _offset);
    }

    function readUInt32(bytes memory _data, uint _offset) internal pure returns (uint new_offset, uint32 r) {
        new_offset = _offset + 4;
        r = bytesToUInt32(_data, _offset);
    }

    function readUInt128(bytes memory _data, uint _offset) internal pure returns (uint new_offset, uint128 r) {
        new_offset = _offset + 16;
        r = bytesToUInt128(_data, _offset);
    }

    function readUInt160(bytes memory _data, uint _offset) internal pure returns (uint new_offset, uint160 r) {
        new_offset = _offset + 20;
        r = bytesToUInt160(_data, _offset);
    }

    function readAddress(bytes memory _data, uint _offset) internal pure returns (uint new_offset, address r) {
        new_offset = _offset + 20;
        r = bytesToAddress(_data, _offset);
    }

    function readBytes20(bytes memory _data, uint _offset) internal pure returns (uint new_offset, bytes20 r) {
        new_offset = _offset + 20;
        r = bytesToBytes20(_data, _offset);
    }

    function readBytes32(bytes memory _data, uint _offset) internal pure returns (uint new_offset, bytes32 r) {
        new_offset = _offset + 32;
        r = bytesToBytes32(_data, _offset);
    }

    // Helper function for hex conversion.
    function halfByteToHex(byte _byte) internal pure returns (byte _hexByte) {
        require(uint8(_byte) | 0xf == 0xf, "hbh11");  // half byte's value is out of 0..15 range.

        // "FEDCBA9876543210" ASCII-encoded, shifted and automatically truncated.
        return byte (uint8 (0x66656463626139383736353433323130 >> (uint8 (_byte) * 8)));
    }

    // Convert bytes to ASCII hex representation
    function bytesToHexASCIIBytes(bytes memory  _input) internal pure returns (bytes memory _output) {
        bytes memory outStringBytes = new bytes(_input.length * 2);
        for (uint i = 0; i < _input.length; ++i) {
            outStringBytes[i*2] = halfByteToHex(_input[i] >> 4);
            outStringBytes[i*2+1] = halfByteToHex(_input[i] & 0x0f);
        }
        return outStringBytes;
    }

    /// Trim bytes into single word
    function trim(bytes memory _data, uint _new_length) internal pure returns (uint r) {
        require(_new_length <= 0x20, "trm10");  // new_length is longer than word
        require(_data.length >= _new_length, "trm11");  // data is to short

        uint a;
        assembly {
            a := mload(add(_data, 0x20)) // load bytes into uint256
        }

        return a >> ((0x20 - _new_length) * 8);
    }
}
