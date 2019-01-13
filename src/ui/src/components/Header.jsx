import React from 'react'
import 'style/header.scss'

const Header = props => (
  <header>
    <div className='title'>
      <h1> {props.title} </h1>
    </div>
  </header>
)

export default Header
